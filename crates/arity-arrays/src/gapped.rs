//! [`GappedArray`]: a pointer-sized, heap-backed array that keeps spare
//! capacity and allows gaps so deletes never move and inserts rarely move.

use alloc::alloc::alloc;
use alloc::alloc::handle_alloc_error;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;

use arity_bitmap::Bitmap;

use crate::Arity;

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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by insert/grow logic in a later task")
)]
fn pow2_cap_for<A: Arity>(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        n.next_power_of_two().min(A::LEN)
    }
}

/// Exponent of a power-of-two capacity (`cap == 1 << cap_exp`). Precondition:
/// `cap` is a power of two `≥ 1`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by alloc_block callers in a later task")
)]
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by insert/rebalance logic in a later task")
)]
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
#[expect(
    dead_code,
    reason = "used by accessor and mutator methods in later tasks"
)]
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
#[expect(dead_code, reason = "used by insert and grow logic in later tasks")]
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

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::Arity16;
    use crate::Arity256;

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
        let _ = GappedArray::<u32, Arity256>::default();
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
