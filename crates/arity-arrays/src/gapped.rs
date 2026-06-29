//! [`GappedArray`]: a pointer-sized, heap-backed array that keeps spare
//! capacity and allows gaps so deletes never move and inserts rarely move.

use alloc::alloc::alloc;
use alloc::alloc::dealloc;
use alloc::alloc::handle_alloc_error;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;

use arity_bitmap::Bitmap;
use arity_index::Niche;

use crate::Arity;
use crate::FixedArray;

/// Heap header: two bitmaps and the capacity exponent, followed (after
/// alignment padding) by the element array. `#[repr(C)]` makes `data` the
/// canonical element-array address.
///
/// `occupancy` is logical membership over `[0, N)`; `live` marks filled
/// physical slots over `[0, capacity)`. They have equal popcount (`count`); the
/// physical slot of logical index `i` is `select(live, rank(occupancy, i))`.
#[repr(C)]
struct Inner<A: Arity, T> {
    occupancy: A::Bitmap,
    live: A::Bitmap,
    /// `capacity == 1 << cap_exp`; always a power of two in `[1, N]`.
    cap_exp: u8,
    /// Zero-sized anchor for the trailing element array (`&raw mut (*p).data`).
    data: [T; 0],
}

/// A pointer-sized, heap-backed array over arity `A` with spare capacity and
/// gaps.
///
/// Trades memory for mutation throughput: it keeps `capacity ≥ count`, grows
/// geometrically by powers of two (bounded by `A::LEN`), and stores present
/// elements in ascending logical order with gaps so deletes are move-free and
/// inserts move only to reach a nearby hole.
///
/// `None` ↔ unallocated (`count == 0`, `capacity == 0`); pointer-sized via the
/// `NonNull` niche. `Some(ptr)` ↔ a live allocation of `capacity` slots, of
/// which `count == occupancy.count_ones() == live.count_ones() ∈ [0, capacity]`
/// are filled. Unlike [`PackedArray`](crate::PackedArray), `Some` with
/// `count == 0` is legal — removing all elements retains the allocation
/// (shrinks are never automatic).
///
/// # Safety
///
/// Invariant: when `self.0` is `Some(ptr)`, `ptr` is a live allocation from
/// `alloc_layout::<A, T>(1 << cap_exp)` whose header is initialised, with
/// `occupancy.count_ones() == live.count_ones()`, the set bits of `live` being
/// exactly the initialised element slots, and the `r`-th set bit of `live`
/// holding the `r`-th present logical index (ascending). Every physical slot
/// read in this module relies on this.
pub struct GappedArray<T, A: Arity>(
    Option<NonNull<Inner<A, T>>>,
    PhantomData<alloc::boxed::Box<T>>,
);

// Compile-time guarantee: pointer-sized. The property is generic over `A`; the
// witness is whichever arity is enabled (mirrors `packed.rs`'s `SizeWitness`
// chain so the assertion fires under any non-empty feature subset).
#[cfg(feature = "8")]
type SizeWitness = crate::Arity8;
#[cfg(all(not(feature = "8"), feature = "16"))]
type SizeWitness = crate::Arity16;
#[cfg(all(not(feature = "8"), not(feature = "16"), feature = "32"))]
type SizeWitness = crate::Arity32;
#[cfg(all(
    not(feature = "8"),
    not(feature = "16"),
    not(feature = "32"),
    feature = "64"
))]
type SizeWitness = crate::Arity64;
#[cfg(all(
    not(feature = "8"),
    not(feature = "16"),
    not(feature = "32"),
    not(feature = "64"),
    feature = "128"
))]
type SizeWitness = crate::Arity128;
#[cfg(all(
    not(feature = "8"),
    not(feature = "16"),
    not(feature = "32"),
    not(feature = "64"),
    not(feature = "128"),
    feature = "256"
))]
type SizeWitness = crate::Arity256;

#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128",
    feature = "256"
))]
const _: () = assert!(
    core::mem::size_of::<GappedArray<[u8; 32], SizeWitness>>() == core::mem::size_of::<*const ()>()
);

/// Smallest power-of-two capacity that holds `n` elements, capped at `A::LEN`.
/// Returns `0` for `n == 0` (the unallocated state). `A::LEN` is itself a power
/// of two, so the cap preserves the power-of-two property.
fn pow2_cap_for<A: Arity>(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        n.next_power_of_two().min(A::LEN)
    }
}

/// Exponent of a power-of-two capacity (`cap == 1 << cap_exp`). Precondition:
/// `cap` is a power of two `≥ 1`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "cap <= A::LEN <= 256, so trailing_zeros() <= 8 fits u8"
)]
fn cap_exp_of(cap: usize) -> u8 {
    debug_assert!(cap.is_power_of_two());
    cap.trailing_zeros() as u8
}

/// Even-spread physical slot for the rank-`r` element among `count` live
/// elements across `cap` slots (`floor(r * cap / count)`, `usize` arithmetic).
/// Precondition: `0 < count <= cap`, `r < count`.
fn spread_pos(r: usize, count: usize, cap: usize) -> usize {
    debug_assert!(0 < count && count <= cap && r < count);
    r * cap / count
}

/// Layout of the heap block for `cap` element slots.
fn alloc_layout<A: Arity, T>(cap: usize) -> Layout {
    let (layout, _) = Layout::new::<Inner<A, T>>()
        .extend(Layout::array::<T>(cap).expect("element layout overflow"))
        .expect("block layout overflow");
    layout.pad_to_align()
}

/// Base address of the element array.
///
/// # Safety
/// `inner` must point to a live allocation from `alloc_layout::<A, T>(cap)`
/// with the header initialised.
unsafe fn data_ptr<A: Arity, T>(inner: NonNull<Inner<A, T>>) -> *mut T {
    // SAFETY: `inner` is valid per the precondition; `#[repr(C)]` places `data`
    // at the element-array offset.
    unsafe { (&raw mut (*inner.as_ptr()).data).cast::<T>() }
}

/// Allocates a block for `cap` slots and writes the header, leaving all element
/// slots uninitialised. Returns the base `Inner`.
///
/// # Safety
/// `cap == 1 << cap_exp` and `cap > 0`. The caller initialises exactly the
/// element slots whose bit is set in `live` before any read, and owns the
/// allocation thereafter.
unsafe fn alloc_block<A: Arity, T>(
    occupancy: A::Bitmap,
    live: A::Bitmap,
    cap_exp: u8,
    cap: usize,
) -> NonNull<Inner<A, T>> {
    let layout = alloc_layout::<A, T>(cap);
    // SAFETY: `cap > 0` so `layout.size() > 0`; null is handled below.
    let Some(raw) = NonNull::new(unsafe { alloc(layout) }) else {
        handle_alloc_error(layout)
    };
    let inner = raw.cast::<Inner<A, T>>();
    // SAFETY: `inner` is freshly allocated and sized for `Inner<A, T>`; writing
    // the three header fields initialises the header before any element.
    unsafe {
        (&raw mut (*inner.as_ptr()).occupancy).write(occupancy);
        (&raw mut (*inner.as_ptr()).live).write(live);
        (&raw mut (*inner.as_ptr()).cap_exp).write(cap_exp);
    }
    inner
}

impl<T, A: Arity> GappedArray<T, A> {
    /// Creates an empty `GappedArray` (no allocation).
    #[must_use]
    pub const fn new() -> Self {
        Self(None, PhantomData)
    }

    /// Returns `true` if there are no present elements. Note: an allocation may
    /// still be retained (capacity is not released on removal).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Returns the allocated slot count (`0` when unallocated).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.0.map_or(0, |ptr| {
            // SAFETY: `Some` ↔ a live allocation with an initialised header.
            1usize << unsafe { ptr.as_ref().cap_exp }
        })
    }

    /// Returns the logical-membership bitmap (`A::Bitmap::ZERO` when empty).
    #[must_use]
    pub fn bitmap(&self) -> A::Bitmap {
        self.0.map_or(A::Bitmap::ZERO, |ptr| {
            // SAFETY: `Some` ↔ a live allocation with an initialised header.
            unsafe { ptr.as_ref().occupancy }
        })
    }

    /// Returns the number of present elements.
    #[must_use]
    pub fn count(&self) -> usize {
        self.bitmap().count_ones() as usize
    }

    /// Returns the heap bytes this array owns (`0` when unallocated): the exact
    /// size of the `capacity`-slot block.
    #[must_use]
    pub fn allocated_size(&self) -> usize {
        self.0
            .map_or(0, |_| alloc_layout::<A, T>(self.capacity()).size())
    }
}

impl<T, A: Arity> Default for GappedArray<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Moves each `Some` element of a `FixedArray<Option<T>, A>` into a spread
/// gapped block; `None` slots are dropped.
impl<T, A: Arity> From<FixedArray<Option<T>, A>> for GappedArray<T, A> {
    fn from(src: FixedArray<Option<T>, A>) -> Self {
        // Pass 1 (by ref): compute the occupancy bitmap.
        let mut occupancy = A::Bitmap::ZERO;
        for (i, slot) in &src {
            if slot.is_some() {
                occupancy = occupancy.with_bit(i);
            }
        }
        let count = occupancy.count_ones() as usize;
        if count == 0 {
            return Self::new();
        }
        let cap = pow2_cap_for::<A>(count);
        let cap_exp = cap_exp_of(cap);
        // Build the `live` bitmap from the spread positions.
        let mut live = A::Bitmap::ZERO;
        for r in 0..count {
            let p = spread_pos(r, count, cap);
            // SAFETY-free: p < cap <= N == Index::COUNT.
            let p_idx = <A::Index as Niche>::try_from_usize(p).expect("spread position < cap <= N");
            live = live.with_bit(p_idx);
        }
        // SAFETY: `cap == 1 << cap_exp > 0`; the fill loop below initialises
        // exactly the `live` slots before any read. Moving owned values out of
        // `Some` and dropping `None` run no user code, so there is no
        // partial-init window to guard on unwind.
        let inner = unsafe { alloc_block::<A, T>(occupancy, live, cap_exp, cap) };
        // SAFETY: `inner` valid; `data_ptr` is the element base.
        let dp = unsafe { data_ptr(inner) };
        let mut r = 0usize;
        for (_i, slot) in src {
            if let Some(v) = slot {
                let p = spread_pos(r, count, cap);
                // SAFETY: `p < cap`; `dp.add(p)` is an uninitialised in-bounds
                // slot whose `live` bit is set; `write` initialises it.
                unsafe { dp.add(p).write(v) };
                r += 1;
            }
        }
        Self(Some(inner), PhantomData)
    }
}

/// Drops the live elements at the set bits of `remaining`, each exactly once.
/// If an element's destructor panics, the not-yet-dropped elements are still
/// dropped: before dropping the element at bit `i`, a guard over the remaining
/// (post-`i`) bits is armed whose `Drop` recurses here. On success the guard is
/// forgotten. Mirrors the slice-drop glue `PackedArray` relies on. Recursion
/// depth is bounded by `count` (≤ 256).
///
/// # Safety
/// `dp` is the element base of a live allocation and every set bit of
/// `remaining` indexes an initialised, not-yet-dropped element.
unsafe fn drop_live_elems<A: Arity, T>(dp: *mut T, mut remaining: A::Bitmap) {
    struct Rest<A: Arity, T> {
        dp: *mut T,
        remaining: A::Bitmap,
    }
    impl<A: Arity, T> Drop for Rest<A, T> {
        fn drop(&mut self) {
            // SAFETY: forwarded contract — `dp` is the element base and the
            // set bits of `remaining` are initialised, not-yet-dropped slots.
            unsafe { drop_live_elems::<A, T>(self.dp, self.remaining) };
        }
    }
    while let Some(i) = remaining.bits().next() {
        remaining = remaining.without_bit(i);
        let guard = Rest::<A, T> { dp, remaining };
        // SAFETY: `i` was a set bit of the live set ⇒ an initialised element;
        // cleared from `remaining` above and not covered by `guard`, so it is
        // dropped exactly once.
        unsafe { core::ptr::drop_in_place(dp.add(i.as_usize())) };
        core::mem::forget(guard);
    }
}

impl<T, A: Arity> Drop for GappedArray<T, A> {
    fn drop(&mut self) {
        // Free the block no matter what — armed first so it runs even if an
        // element destructor unwinds through `drop_live_elems`.
        struct FreeOnDrop<A: Arity, T> {
            ptr: NonNull<Inner<A, T>>,
            cap: usize,
        }
        impl<A: Arity, T> Drop for FreeOnDrop<A, T> {
            fn drop(&mut self) {
                // SAFETY: sole dealloc of a block from `alloc_layout(cap)`.
                unsafe { dealloc(self.ptr.as_ptr().cast(), alloc_layout::<A, T>(self.cap)) };
            }
        }
        let Some(ptr) = self.0 else { return };
        // SAFETY: `ptr` valid per the invariant; header initialised.
        let live = unsafe { ptr.as_ref().live };
        let cap = self.capacity();
        // SAFETY: `ptr` valid; `data_ptr` is the element base.
        let dp = unsafe { data_ptr(ptr) };
        let _free = FreeOnDrop::<A, T> { ptr, cap };
        // SAFETY: `live` marks the initialised slots; drop them all (re-arm drops
        // the rest on panic), then `_free` deallocs as this scope unwinds/returns.
        unsafe { drop_live_elems::<A, T>(dp, live) };
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::Arity16;
    use crate::Arity256;
    use crate::index::U4;

    #[test]
    fn empty_is_pointer_sized_and_inert() {
        assert_eq!(
            core::mem::size_of::<GappedArray<[u8; 32], Arity16>>(),
            core::mem::size_of::<*const ()>()
        );
        let g = GappedArray::<u8, Arity16>::new();
        assert!(g.is_empty());
        assert_eq!(g.count(), 0);
        assert_eq!(g.capacity(), 0);
        assert_eq!(g.allocated_size(), 0);
        assert_eq!(g.bitmap(), <u16 as arity_bitmap::Bitmap>::ZERO);
        drop(GappedArray::<u32, Arity256>::default());
    }

    #[test]
    fn from_fixed_spreads_and_drops() {
        let mut src = FixedArray::<Option<u16>, Arity16>::new();
        src[U4::new_masked(0)] = Some(10);
        src[U4::new_masked(7)] = Some(70);
        src[U4::new_masked(15)] = Some(150);
        let g = GappedArray::from(src);
        assert_eq!(g.count(), 3);
        // capacity is the next power of two >= count.
        assert_eq!(g.capacity(), 4);
        assert_eq!(g.bitmap().count_ones(), 3);
        // empty source -> unallocated.
        let empty = GappedArray::<u16, Arity16>::from(FixedArray::<Option<u16>, Arity16>::new());
        assert!(empty.is_empty());
        assert_eq!(empty.capacity(), 0);
    }

    #[test]
    fn drop_runs_once_per_element() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        struct Counted(Arc<AtomicUsize>);
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let c = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Counted>, Arity16>::new();
        src[U4::new_masked(2)] = Some(Counted(c.clone()));
        src[U4::new_masked(9)] = Some(Counted(c.clone()));
        let g = GappedArray::from(src);
        assert_eq!(c.load(Ordering::SeqCst), 0);
        drop(g);
        assert_eq!(c.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn drop_panic_drops_rest_and_frees() {
        use std::panic;
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        // The 2nd element to drop panics; all four must still be dropped and the
        // block freed (Miri/ASAN would flag a leak otherwise).
        struct Bomb {
            drops: Arc<AtomicUsize>,
            boom_at: usize,
        }
        impl Drop for Bomb {
            fn drop(&mut self) {
                let n = self.drops.fetch_add(1, Ordering::SeqCst);
                assert!(n != self.boom_at, "boom");
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Bomb>, Arity16>::new();
        for s in 0..4u8 {
            src[U4::new_masked(s)] = Some(Bomb {
                drops: drops.clone(),
                boom_at: 1,
            });
        }
        let g = GappedArray::from(src);
        let r = panic::catch_unwind(panic::AssertUnwindSafe(|| drop(g)));
        assert!(r.is_err());
        // All four destructors ran despite the panic on the second.
        assert_eq!(drops.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn capacity_helpers() {
        assert_eq!(pow2_cap_for::<Arity16>(0), 0);
        assert_eq!(pow2_cap_for::<Arity16>(1), 1);
        assert_eq!(pow2_cap_for::<Arity16>(3), 4);
        assert_eq!(pow2_cap_for::<Arity16>(16), 16);
        assert_eq!(pow2_cap_for::<Arity16>(99), 16); // capped at N
        assert_eq!(pow2_cap_for::<Arity256>(200), 256);
        assert_eq!(cap_exp_of(1), 0);
        assert_eq!(cap_exp_of(16), 4);
        assert_eq!(cap_exp_of(256), 8);
        assert_eq!(spread_pos(0, 3, 8), 0);
        assert_eq!(spread_pos(1, 3, 8), 2);
        assert_eq!(spread_pos(2, 3, 8), 5);
        // dense when count == cap
        assert_eq!(spread_pos(2, 4, 4), 2);
    }
}
