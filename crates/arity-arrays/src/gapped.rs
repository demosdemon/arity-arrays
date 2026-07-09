//! [`GappedArray`]: a pointer-sized, heap-backed array that keeps spare
//! capacity and allows gaps so deletes never move and inserts rarely move.

use alloc::alloc::alloc;
use alloc::alloc::dealloc;
use alloc::alloc::handle_alloc_error;
use core::marker::PhantomData;
use core::ptr::NonNull;

use arity_bitmap::Bitmap;
use arity_index::Niche;

use crate::Arity;
use crate::FixedArray;
use crate::PackedArray;
use crate::raw::alloc_layout;

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
/// # Two-bitmap model
///
/// Each `GappedArray` heap block holds two bitmaps of type `A::Bitmap` in its
/// header:
///
/// - **`occupancy`** — logical membership over `[0, N)`. Bit `i` is set when
///   logical index `i` has a present element.
/// - **`live`** — physical fill over `[0, capacity)`. Bit `p` is set when
///   physical slot `p` of the element array is initialised.
///
/// Both bitmaps always have equal popcount (`count`). The `r`-th set bit of
/// `occupancy` names the logical index of the `r`-th present element; the
/// `r`-th set bit of `live` gives its physical slot. Their rank order is always
/// consistent: the physical slot of logical index `i` is
/// `live.select(occupancy.rank(i))`.
///
/// # Capacity and exponent
///
/// Capacity is always a power of two, stored as an exponent `cap_exp` such that
/// `capacity == 1 << cap_exp`, bounded by `A::LEN`. It starts at 1 on the first
/// insert and doubles when the array is full. The exponent representation fits
/// in one byte (since `A::LEN ≤ 256 = 1 << 8`) and makes doubling a single byte
/// increment.
///
/// # Gap invariants
///
/// **Delete never moves.** Removing an element clears both its `occupancy` bit
/// and its `live` slot bit, then reads the value out — no other element moves.
///
/// **Insert moves only when forced, and prefers the cheapest move.** A new
/// absent element is placed by the lowest-cost strategy available:
///
/// 1. **Hole between neighbors** — if a hole exists strictly between the
///    predecessor and successor physical slots, write the element there (zero
///    element moves).
/// 2. **Shift to nearest hole** — if no such hole exists, slide the shorter of
///    the left or right live run to an adjacent hole (linear in run length, not
///    total occupancy).
/// 3. **Full respread** — rebuild the block with all elements at even-spread
///    positions. This is the full-block path: when `count == cap`, the insert
///    doubles capacity and respreads to include the new element. With spare
///    capacity a respread arm also exists as a cost tie-break, but it never
///    wins there — a shift crosses at most `count` live elements, so its cost
///    is always ≤ the respread cost of `count`, and ties favor a shift. So
///    respread is reached only on a full block.
///
/// # Allocation model
///
/// `None` ↔ unallocated (`count == 0`, `capacity == 0`); pointer-sized via the
/// `NonNull` niche. `Some(ptr)` ↔ a live allocation of `capacity` slots, of
/// which `count == occupancy.count_ones() == live.count_ones() ∈ [0, capacity]`
/// are filled. Unlike [`PackedArray`], `Some` with `count == 0` is legal —
/// removing all elements retains the allocation (shrinks are never automatic;
/// call [`shrink_to_fit`] or convert to [`PackedArray`] via `From` for the
/// exact-fit escape hatch).
///
/// # Trait implementations
///
/// [`PartialEq`], [`Eq`], [`core::hash::Hash`], and [`core::fmt::Debug`] are
/// all gap-agnostic: two arrays with the same logical content (same bitmap,
/// same values in the same order) compare equal and hash identically regardless
/// of capacity or hole placement.
///
/// `Ord` and `PartialOrd` are intentionally **not** implemented: a sparse map
/// has no canonical total order, and adding them later is non-breaking.
///
/// # Safety: physical vs. logical index domain
///
/// This module uses two distinct index domains that share the `A::Index` type:
///
/// - **Logical** — values produced by `occupancy` bit operations and exposed to
///   callers; range `[0, N)`.
/// - **Physical** — slot offsets into the element array; range `[0, capacity)`.
///
/// `live.select(r)` returns an `Option<A::Index>` representing a **physical**
/// slot offset. Its `.as_usize()` yields the `usize` used to address the
/// element array (`data_ptr(ptr).add(p)`). Converting a physical position `p`
/// back to `A::Index` via
/// `<A::Index as Niche>::try_from_usize(p).expect("p < capacity ≤ N")` is sound
/// because `capacity ≤ A::LEN == A::Index::COUNT`, guaranteeing `p` is in
/// range.
///
/// Structural invariant: when `self.0` is `Some(ptr)`, `ptr` is a live
/// allocation from `alloc_layout::<Inner<A, T>, T>(1 << cap_exp)` whose header
/// is initialised with `occupancy.count_ones() == live.count_ones()`, the set
/// bits of `live` being exactly the initialised element slots, and the `r`-th
/// set bit of `live` (in ascending order) holding the physical slot of the
/// `r`-th present element. Every physical slot read in this module relies on
/// this invariant.
///
/// # Example
///
/// ```
/// # extern crate alloc;
/// use arity_arrays::{Arity16, GappedArray};
/// use arity_arrays::index::{Niche, U4};
///
/// // Mutations operate in place: inserts open nearby gaps; deletes leave holes.
/// let mut g = GappedArray::<u32, Arity16>::new();
/// g.insert(U4::new_masked(1), 10);
/// g.insert(U4::new_masked(9), 90);
/// assert_eq!(g.count(), 2);
/// assert_eq!(g.get(U4::new_masked(9)), Some(&90));
/// assert_eq!(g.get(U4::new_masked(0)), None);
///
/// let present: alloc::vec::Vec<(u8, u32)> =
///     g.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
/// assert_eq!(present, alloc::vec![(1, 10), (9, 90)]);
/// ```
///
/// [`shrink_to_fit`]: GappedArray::shrink_to_fit
pub struct GappedArray<T, A: Arity>(
    Option<NonNull<Inner<A, T>>>,
    PhantomData<alloc::boxed::Box<T>>,
);

impl_size_witness!(GappedArray);

/// Smallest power-of-two capacity that holds `n` elements, capped at `A::LEN`.
/// Returns `0` for `n == 0` (the unallocated state). `A::LEN` is itself a power
/// of two, so the cap preserves the power-of-two property. Safe for large `n`
/// (including `usize::MAX`): caps before calling `next_power_of_two` to avoid
/// overflow for values that already meet or exceed `A::LEN`.
const fn pow2_cap_for<A: Arity>(n: usize) -> usize {
    if n == 0 {
        0
    } else if n >= A::LEN {
        A::LEN
    } else {
        n.next_power_of_two()
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

/// Base address of the element array.
///
/// # Safety
/// `inner` must point to a live allocation from `alloc_layout::<Inner<A, T>,
/// T>(cap)` with the header initialised.
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
    let layout = alloc_layout::<Inner<A, T>, T>(cap);
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
        self.0.map_or(0, |_| {
            alloc_layout::<Inner<A, T>, T>(self.capacity()).size()
        })
    }

    /// Returns a reference to the element at `index`, or `None` if absent.
    ///
    /// # Panics
    ///
    /// Panics if the internal bitmap invariant is violated (i.e., `occupancy`
    /// and `live` have mismatched popcount). This cannot happen through the
    /// public API.
    #[must_use]
    pub fn get(&self, index: A::Index) -> Option<&T> {
        let ptr = self.0?;
        // SAFETY: `ptr` valid per the invariant.
        let occ = unsafe { ptr.as_ref().occupancy };
        if !occ.test(index) {
            return None;
        }
        // SAFETY: `ptr` valid per the invariant.
        let live = unsafe { ptr.as_ref().live };
        let r = occ.rank(index);
        let p = live
            .select(r)
            .expect("present ⇒ rank < count == live popcount")
            .as_usize();
        // SAFETY: `p` is a set `live` bit (a physical slot < cap), so
        // `data_ptr(ptr).add(p)` is an initialised element.
        Some(unsafe { &*data_ptr(ptr).add(p) })
    }

    /// Returns a mutable reference to the element at `index`, or `None` if
    /// absent. Does not change which slots are present.
    ///
    /// # Panics
    ///
    /// Panics if the internal bitmap invariant is violated (i.e., `occupancy`
    /// and `live` have mismatched popcount). This cannot happen through the
    /// public API.
    pub fn get_mut(&mut self, index: A::Index) -> Option<&mut T> {
        let ptr = self.0?;
        // SAFETY: `ptr` valid per the invariant.
        let occ = unsafe { ptr.as_ref().occupancy };
        if !occ.test(index) {
            return None;
        }
        // SAFETY: `ptr` valid per the invariant.
        let live = unsafe { ptr.as_ref().live };
        let r = occ.rank(index);
        let p = live.select(r).expect("present ⇒ rank < count").as_usize();
        // SAFETY: `p` is a physical slot < cap with an initialised element;
        // the borrow is tied to `&mut self`, giving exclusive access.
        Some(unsafe { &mut *data_ptr(ptr).add(p) })
    }

    /// Removes and returns the element at `index`, or `None` if absent.
    ///
    /// Move-free: clears the membership/live bits and reads the value out,
    /// leaving a hole. The allocation is retained even when the array becomes
    /// empty (shrinks are never automatic; use [`shrink_to_fit`] or convert).
    ///
    /// [`shrink_to_fit`]: GappedArray::shrink_to_fit
    ///
    /// # Panics
    ///
    /// Panics if the internal bitmap invariant is violated (i.e., `occupancy`
    /// and `live` have mismatched popcount). This cannot happen through the
    /// public API.
    pub fn remove(&mut self, index: A::Index) -> Option<T> {
        let ptr = self.0?;
        // SAFETY: `ptr` valid per the invariant.
        let occ = unsafe { ptr.as_ref().occupancy };
        if !occ.test(index) {
            return None;
        }
        // SAFETY: `ptr` valid per the invariant.
        let live = unsafe { ptr.as_ref().live };
        let r = occ.rank(index);
        let p = live.select(r).expect("present ⇒ rank < count").as_usize();
        let p_idx = <A::Index as Niche>::try_from_usize(p).expect("p < cap <= N");
        // Clear bits FIRST (bitmap ops cannot panic). This closes the
        // double-drop window: if the returned value's destructor later panics
        // in the caller, `Drop` already sees `live[p]` clear.
        // SAFETY: `ptr` valid; the header fields are initialised and `Copy`.
        unsafe {
            (*ptr.as_ptr()).occupancy = occ.without_bit(index);
            (*ptr.as_ptr()).live = live.without_bit(p_idx);
        }
        // SAFETY: `p` was a set live bit; `read` moves the element out without
        // dropping it (returned to the caller). The slot is now logically
        // uninitialised and its live bit is clear, so it is never touched again.
        Some(unsafe { data_ptr(ptr).add(p).read() })
    }

    /// Iterates all `A::LEN` slots as `(A::Index, Option<&T>)`, ascending.
    /// Double-ended.
    #[must_use]
    pub fn iter(&self) -> GappedAllIter<'_, T, A> {
        GappedAllIter {
            present: self.iter_present(),
            bitmap: self.bitmap(),
            slots: <A::Index as Niche>::all(),
        }
    }

    /// Reallocates to a fresh `new_cap`-slot block with all live elements
    /// re-laid at even spread positions, then frees the old block. Used by grow
    /// and respread. `new_cap` must be a power of two `≥ count`. No-op shape
    /// when unallocated with `new_cap` chosen for `count == 0` is not allowed;
    /// callers handle the empty case separately.
    fn rebuild_to(&mut self, new_cap: usize) {
        let Some(old_ptr) = self.0 else { return };
        // SAFETY: `old_ptr` valid per the invariant.
        let occ = unsafe { old_ptr.as_ref().occupancy };
        // SAFETY: `old_ptr` valid per the invariant.
        let old_live = unsafe { old_ptr.as_ref().live };
        let old_cap = self.capacity();
        let count = occ.count_ones() as usize;
        debug_assert!(count <= new_cap && new_cap.is_power_of_two());
        let new_cap_exp = cap_exp_of(new_cap);
        // New live bitmap from the fresh spread positions.
        let mut new_live = A::Bitmap::ZERO;
        for r in 0..count {
            let p = spread_pos(r, count, new_cap);
            let p_idx = <A::Index as Niche>::try_from_usize(p).expect("p < new_cap <= N");
            new_live = new_live.with_bit(p_idx);
        }
        // SAFETY: `new_cap >= 1` because callers only pass `new_cap > capacity >= 1`
        // (an allocated block has capacity >= 1); the 0..count copy loops degenerate
        // harmlessly when count == 0. The copy initialises exactly the `new_live`
        // slots before any read.
        let new_ptr = unsafe { alloc_block::<A, T>(occ, new_live, new_cap_exp, new_cap) };
        // SAFETY: `old_ptr` valid per the invariant; distinct from `new_ptr`.
        let src = unsafe { data_ptr(old_ptr) };
        // SAFETY: `new_ptr` is the freshly allocated block above.
        let dst = unsafe { data_ptr(new_ptr) };
        // Copy the r-th live element from its old physical slot to its new one.
        // The r-th set bit of `new_live` is the rank-r spread position, so
        // reading it here avoids recomputing `spread_pos` per element.
        for (old_i, new_i) in old_live.bits().zip(new_live.bits()) {
            let op = old_i.as_usize();
            let np = new_i.as_usize();
            // SAFETY: `op` is a set old-live slot (initialised); `np` is a set
            // new-live slot (`< new_cap`). Distinct allocations ⇒ non-overlapping
            // bitwise move (no drop, no user code).
            unsafe { core::ptr::copy_nonoverlapping(src.add(op), dst.add(np), 1) };
        }
        self.0 = Some(new_ptr);
        // SAFETY: live elements moved out; old block came from
        // `alloc_layout(old_cap)`; freed exactly once.
        unsafe {
            dealloc(
                old_ptr.as_ptr().cast(),
                alloc_layout::<Inner<A, T>, T>(old_cap),
            );
        };
    }

    /// Replaces the block with a fresh `new_cap`-slot block holding all current
    /// elements **plus** the new `(index, value)`, each at its spread position
    /// for the new count. This is "full respread including the new element": it
    /// always succeeds (no hole search), so it is the correct fallback when no
    /// between-neighbors hole exists and the grow path's placement. Requires
    /// `index` absent and `count + 1 <= new_cap`, `new_cap` a power of two.
    ///
    /// Panic-safe: the only fallible step is `alloc_block` (before any move);
    /// element relocation is `copy_nonoverlapping` (bitwise, no drop/user code)
    /// and the new value is written by move, so no fill guard is needed.
    fn rebuild_with_insert(&mut self, index: A::Index, value: T, new_cap: usize) {
        let old_ptr = self.0.expect("rebuild_with_insert requires an allocation");
        // SAFETY: `old_ptr` valid per the invariant.
        let occ = unsafe { old_ptr.as_ref().occupancy };
        // SAFETY: `old_ptr` valid per the invariant.
        let old_live = unsafe { old_ptr.as_ref().live };
        let old_cap = self.capacity();
        debug_assert!(!occ.test(index));
        let new_occ = occ.with_bit(index);
        let new_count = new_occ.count_ones() as usize; // == count + 1
        debug_assert!(new_count <= new_cap && new_cap.is_power_of_two());
        let new_rank = new_occ.rank(index) as usize; // rank of the inserted index
        let new_cap_exp = cap_exp_of(new_cap);
        // New live bitmap from the spread of `new_count` across `new_cap`.
        let mut new_live = A::Bitmap::ZERO;
        for r in 0..new_count {
            let p = spread_pos(r, new_count, new_cap);
            let p_idx = <A::Index as Niche>::try_from_usize(p).expect("p < new_cap <= N");
            new_live = new_live.with_bit(p_idx);
        }
        // SAFETY: `new_cap > 0`; the copies + the write below initialise exactly
        // the `new_live` slots before any read.
        let new_ptr = unsafe { alloc_block::<A, T>(new_occ, new_live, new_cap_exp, new_cap) };
        // SAFETY: `old_ptr` valid per the invariant; distinct from `new_ptr`.
        let src = unsafe { data_ptr(old_ptr) };
        // SAFETY: `new_ptr` is the freshly allocated block above.
        let dst = unsafe { data_ptr(new_ptr) };
        // Fill each new slot in rank order, reading destinations from `new_live`
        // (its r-th set bit is the rank-r spread position) instead of recomputing
        // `spread_pos`. The inserted rank takes the new value by move; every
        // other rank takes the next old element in order.
        let mut value = Some(value);
        let mut old_slots = old_live.bits();
        for (nr, new_i) in new_live.bits().enumerate() {
            let np = new_i.as_usize();
            if nr == new_rank {
                // SAFETY: `np < new_cap` is the fresh slot for the inserted rank,
                // visited exactly once (so `take` yields the value).
                unsafe {
                    dst.add(np)
                        .write(value.take().expect("inserted rank visited once"));
                }
            } else {
                let op = old_slots
                    .next()
                    .expect("an old element for each non-inserted rank")
                    .as_usize();
                // SAFETY: `op` is a set old-live slot (initialised); `np < new_cap`
                // is a fresh slot in a distinct allocation; bitwise move, no drop.
                unsafe { core::ptr::copy_nonoverlapping(src.add(op), dst.add(np), 1) };
            }
        }
        self.0 = Some(new_ptr);
        // SAFETY: all old elements moved out; old block from `alloc_layout(old_cap)`.
        unsafe {
            dealloc(
                old_ptr.as_ptr().cast(),
                alloc_layout::<Inner<A, T>, T>(old_cap),
            );
        };
    }

    /// Inserts `value` at `index`, returning the previous value if the slot was
    /// already present.
    ///
    /// Overwrite is in place. A new insertion fills a hole between its
    /// rank-neighbors when one exists; otherwise capacity grows (doubling, a
    /// power of two bounded by `A::LEN`) or the block is respread to open a
    /// gap.
    ///
    /// # Panics
    ///
    /// Panics if the internal bitmap invariant is violated (i.e., `occupancy`
    /// and `live` have mismatched popcount). This cannot happen through the
    /// public API.
    pub fn insert(&mut self, index: A::Index, value: T) -> Option<T> {
        // Empty → fresh single-slot block.
        let Some(ptr) = self.0 else {
            let occ = A::Bitmap::ZERO.with_bit(index);
            let live =
                A::Bitmap::ZERO.with_bit(<A::Index as Niche>::try_from_usize(0).expect("0 < N"));
            // SAFETY: cap == 1 > 0; the write below initialises slot 0 (the sole
            // live bit) before any read.
            let inner = unsafe { alloc_block::<A, T>(occ, live, 0, 1) };
            // SAFETY: `inner` valid; slot 0 is the sole uninitialised element.
            unsafe { data_ptr(inner).write(value) };
            self.0 = Some(inner);
            return None;
        };
        // SAFETY: `ptr` valid per the invariant.
        let occ = unsafe { ptr.as_ref().occupancy };
        // Present → overwrite in place.
        if occ.test(index) {
            // SAFETY: `ptr` valid.
            let live = unsafe { ptr.as_ref().live };
            let r = occ.rank(index);
            let p = live.select(r).expect("present ⇒ rank < count").as_usize();
            // SAFETY: `p` is a set live slot with an initialised element;
            // `&mut self` gives exclusive access.
            return Some(unsafe { core::mem::replace(&mut *data_ptr(ptr).add(p), value) });
        }
        let count = occ.count_ones() as usize;
        let cap = self.capacity();
        // Full → grow (double, bounded by N) and respread *including* the new
        // element. `count < A::LEN` here (an absent index exists), so
        // `pow2_cap_for(count + 1) == 2 * cap` and `count + 1 <= new_cap`.
        if count == cap {
            let new_cap = pow2_cap_for::<A>(count + 1);
            self.rebuild_with_insert(index, value, new_cap);
            return None;
        }
        self.place_absent(index, value);
        None
    }

    /// Places `value` at an absent `index` given spare capacity (`count <
    /// cap`). Fills a hole strictly between the rank-neighbors if one exists;
    /// otherwise chooses the cheapest of shift-left, shift-right, or full
    /// respread (`rebuild_with_insert`, cost ≈ count).
    fn place_absent(&mut self, index: A::Index, value: T) {
        let ptr = self.0.expect("place_absent requires an allocation");
        // Read the header snapshot once and thread it into the bounds/hole
        // searches below, which would otherwise re-read it. The shared reference
        // is dropped here, before the later mutations through `ptr`.
        // SAFETY: `ptr` valid per the invariant.
        let (occ, live, cap) = unsafe {
            let inner = ptr.as_ref();
            (inner.occupancy, inner.live, 1usize << inner.cap_exp)
        };
        let count = occ.count_ones() as usize;
        let (p_lo, p_hi) = Self::neighbor_bounds(occ, live, cap, count, index);
        // Hole strictly between the neighbors → place with no move.
        // `p_lo >= -1` (sentinel) so `p_lo + 1 >= 0`; cast_unsigned is safe.
        let mut p = (p_lo + 1).cast_unsigned();
        while p < p_hi {
            let p_idx = <A::Index as Niche>::try_from_usize(p).expect("p < cap <= N");
            if !live.test(p_idx) {
                self.write_into_hole(index, p, value);
                return;
            }
            p += 1;
        }
        // Neighbors are physically adjacent. Choose the cheapest of: shift the
        // run left to the nearest hole, shift it right to the nearest hole, or
        // respread the whole block including the new element (cost ≈ count).
        // With `count < cap` there is always ≥ 1 hole, so at least one shift is
        // finite; the respread branch is a correct tie-break, never a panic.
        let left = Self::nearest_hole_left(live, p_lo); // Option<(hole_pos, dist)>
        let right = Self::nearest_hole_right(live, p_hi, cap); // Option<(hole_pos, dist)>
        let respread_cost = count;
        let left_cost = left.map_or(usize::MAX, |(_, d)| d);
        let right_cost = right.map_or(usize::MAX, |(_, d)| d);
        if left_cost <= right_cost && left_cost <= respread_cost {
            let (hpos, _) = left.expect("left_cost finite ⇒ Some");
            self.shift_left_and_write(index, hpos, p_hi, value);
        } else if right_cost <= respread_cost {
            let (hpos, _) = right.expect("right_cost finite ⇒ Some");
            self.shift_right_and_write(index, p_hi, hpos, value);
        } else {
            // Reachable only if both shifts are None, which cannot happen for
            // `count < cap`; kept as a safe fallback (cap unchanged, count+1 ≤ cap).
            self.rebuild_with_insert(index, value, cap);
        }
    }

    /// Nearest hole at a physical slot `< from` (scanning down). Returns
    /// `(hole_pos, live_elements_crossed)`. Pure over `live` so the caller's
    /// header snapshot is reused.
    fn nearest_hole_left(live: A::Bitmap, from: isize) -> Option<(usize, usize)> {
        let mut p = from; // from == p_lo (a live slot or -1)
        let mut crossed = 0usize;
        while p >= 0 {
            let p_idx = <A::Index as Niche>::try_from_usize(p.cast_unsigned()).expect("p < cap");
            if !live.test(p_idx) {
                return Some((p.cast_unsigned(), crossed));
            }
            crossed += 1;
            p -= 1;
        }
        None
    }

    /// Nearest hole at a physical slot `>= from` (scanning up to `cap`).
    /// Returns `(hole_pos, live_elements_crossed)`. Pure over `live` so the
    /// caller's header snapshot is reused.
    fn nearest_hole_right(live: A::Bitmap, from: usize, cap: usize) -> Option<(usize, usize)> {
        let mut crossed = 0usize;
        let mut p = from; // from == p_hi (a live slot or cap)
        while p < cap {
            let p_idx = <A::Index as Niche>::try_from_usize(p).expect("p < cap");
            if !live.test(p_idx) {
                return Some((p, crossed));
            }
            crossed += 1;
            p += 1;
        }
        None
    }

    /// Shift the live run `(hole, p_hi)` down by one to open slot `p_hi - 1`,
    /// then write `value` there. `hole < p_hi`, all slots in `(hole, p_hi)`
    /// live.
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "mutation occurs through raw pointer derived from self.0; \
                  clippy cannot see through the raw-pointer write"
    )]
    fn shift_left_and_write(&mut self, index: A::Index, hole: usize, p_hi: usize, value: T) {
        let ptr = self.0.expect("allocation present");
        let n = p_hi - hole - 1; // elements in (hole, p_hi) to shift down by one
        // SAFETY: slots (hole, p_hi) are initialised live elements; copying them
        // one slot toward `hole` is an overlap-safe bitwise move (no drop/user
        // code runs). The vacated slot is `p_hi - 1`, where `value` is written
        // via `ptr::write` (not `replace`): this is not a double-drop because the
        // `ptr::copy` bitwise-moved the run down by one, so the former occupant of
        // `p_hi - 1` now lives at `p_hi - 2`; slot `p_hi - 1` holds a bitwise
        // duplicate that is overwritten (correct — it was moved, not cloned).
        // After the copy, exactly one new physical slot is live — the former
        // hole — so the live bitmap gains `hole_idx` and is otherwise unchanged.
        unsafe {
            let base = data_ptr(ptr);
            core::ptr::copy(base.add(hole + 1), base.add(hole), n);
            let write_at = p_hi - 1;
            base.add(write_at).write(value);
            // occupancy gains `index`; live gains the hole (now filled).
            let occ = (*ptr.as_ptr()).occupancy.with_bit(index);
            let live = (*ptr.as_ptr()).live;
            let hole_idx = <A::Index as Niche>::try_from_usize(hole).expect("hole < cap");
            (*ptr.as_ptr()).occupancy = occ;
            (*ptr.as_ptr()).live = live.with_bit(hole_idx);
        }
    }

    /// Shift the live run `[p_hi, hole)` up by one to open slot `p_hi`, then
    /// write `value` there. `p_hi <= hole`, all slots in `[p_hi, hole)` live.
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "mutation occurs through raw pointer derived from self.0; \
                  clippy cannot see through the raw-pointer write"
    )]
    fn shift_right_and_write(&mut self, index: A::Index, p_hi: usize, hole: usize, value: T) {
        let ptr = self.0.expect("allocation present");
        let n = hole - p_hi; // elements in [p_hi, hole) to shift up by one
        // SAFETY: slots [p_hi, hole) are initialised live elements; copying them
        // up by one is an overlap-safe bitwise move (no drop/user code runs).
        // The vacated slot is `p_hi`, where `value` is written. After the copy,
        // exactly one new physical slot is live — the former hole — so the live
        // bitmap gains `hole_idx` and is otherwise unchanged.
        unsafe {
            let base = data_ptr(ptr);
            core::ptr::copy(base.add(p_hi), base.add(p_hi + 1), n);
            base.add(p_hi).write(value);
            // occupancy gains `index`; live gains the hole (now filled).
            let occ = (*ptr.as_ptr()).occupancy.with_bit(index);
            let live = (*ptr.as_ptr()).live;
            let hole_idx = <A::Index as Niche>::try_from_usize(hole).expect("hole < cap");
            (*ptr.as_ptr()).occupancy = occ;
            (*ptr.as_ptr()).live = live.with_bit(hole_idx);
        }
    }

    /// Physical-slot bounds bracketing the rank of an absent `index`: the
    /// predecessor live slot (`-1` sentinel when the index is the new minimum)
    /// and the successor live slot (`cap` sentinel when it is the new maximum).
    /// Pure over the supplied header snapshot (`occ`, `live`, `cap`, `count`),
    /// so a caller that already read the header does not re-read it.
    fn neighbor_bounds(
        occ: A::Bitmap,
        live: A::Bitmap,
        cap: usize,
        count: usize,
        index: A::Index,
    ) -> (isize, usize) {
        let r = occ.rank(index) as usize; // target rank in [0, count]
        // Physical slots are < cap <= N <= 256, so they fit in isize without wrap.
        let p_lo: isize = if r == 0 {
            -1
        } else {
            live.select(u32::try_from(r - 1).expect("r <= 256 fits u32"))
                .expect("r-1 < count")
                .as_usize()
                .cast_signed()
        };
        let p_hi: usize = if r == count {
            cap
        } else {
            live.select(u32::try_from(r).expect("r <= 256 fits u32"))
                .expect("r < count")
                .as_usize()
        };
        (p_lo, p_hi)
    }

    /// Writes `value` at physical hole `hp` and sets the membership/live bits.
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "mutation occurs through raw pointer derived from self.0; \
                  clippy cannot see through the raw-pointer write"
    )]
    fn write_into_hole(&mut self, index: A::Index, hp: usize, value: T) {
        let ptr = self.0.expect("allocation present");
        // SAFETY: `ptr` valid per the invariant.
        let occ = unsafe { ptr.as_ref().occupancy };
        // SAFETY: `ptr` valid per the invariant.
        let live = unsafe { ptr.as_ref().live };
        let hp_idx = <A::Index as Niche>::try_from_usize(hp).expect("hp < cap <= N");
        debug_assert!(!live.test(hp_idx));
        // SAFETY: `hp` is a hole (uninitialised slot < cap); `write` initialises
        // it. Header fields are initialised and `Copy`.
        unsafe {
            data_ptr(ptr).add(hp).write(value);
            (*ptr.as_ptr()).occupancy = occ.with_bit(index);
            (*ptr.as_ptr()).live = live.with_bit(hp_idx);
        }
    }

    /// Creates an array with capacity for at least `n` elements (rounded up to
    /// a power of two, capped at `A::LEN`). `with_capacity(0)` is the
    /// unallocated state. Eager: the reservation lives in the heap header.
    #[must_use]
    pub fn with_capacity(n: usize) -> Self {
        let cap = pow2_cap_for::<A>(n);
        if cap == 0 {
            return Self::new();
        }
        // SAFETY: `cap > 0`; no element slots are live, so the empty block is
        // fully described by its zeroed bitmaps — nothing to initialise.
        let inner =
            unsafe { alloc_block::<A, T>(A::Bitmap::ZERO, A::Bitmap::ZERO, cap_exp_of(cap), cap) };
        Self(Some(inner), PhantomData)
    }

    /// Ensures capacity for at least `count + n` elements (power of two, capped
    /// at `A::LEN`). Saturating: `n` may be `usize::MAX`.
    pub fn reserve(&mut self, n: usize) {
        let want = self.count().saturating_add(n);
        let target = pow2_cap_for::<A>(want);
        if target <= self.capacity() {
            return;
        }
        match self.0 {
            Some(_) => self.rebuild_to(target),
            None => *self = Self::with_capacity(want),
        }
    }

    /// Shrinks capacity to the smallest power of two ≥ `count` (respread);
    /// becomes unallocated when empty. The only in-type capacity reduction.
    pub fn shrink_to_fit(&mut self) {
        let count = self.count();
        if count == 0 {
            // Drop the (empty) allocation and become `None`.
            *self = Self::new();
            return;
        }
        let target = pow2_cap_for::<A>(count);
        if target < self.capacity() {
            self.rebuild_to(target);
        }
    }

    /// Drops all present elements, retaining the allocation (capacity
    /// unchanged).
    pub fn clear(&mut self) {
        let Some(ptr) = self.0 else { return };
        // SAFETY: `ptr` valid per the invariant.
        let live = unsafe { ptr.as_ref().live };
        // SAFETY: `ptr` valid per the invariant.
        let dp = unsafe { data_ptr(ptr) };
        // Reset membership FIRST so a panicking destructor cannot leave a set
        // live bit that a later `Drop` would double-drop; the allocation stays.
        // SAFETY: header initialised and `Copy`.
        unsafe {
            (*ptr.as_ptr()).occupancy = A::Bitmap::ZERO;
            (*ptr.as_ptr()).live = A::Bitmap::ZERO;
        }
        // SAFETY: `live` are the initialised slots; `drop_live_elems` drops them
        // all (re-arm drops the rest on a panicking destructor). The header is
        // already cleared, so no double-drop is possible.
        unsafe { drop_live_elems::<A, T>(dp, live) };
    }

    /// Iterates present elements as `(A::Index, &T)`, ascending. Double-ended.
    /// `O(1)` per step (co-advances the occupancy and live bit cursors).
    #[must_use]
    pub fn iter_present(&self) -> GappedPresentIter<'_, T, A> {
        self.0.map_or_else(
            || GappedPresentIter {
                occ_bits: A::Bitmap::ZERO.bits(),
                live_bits: A::Bitmap::ZERO.bits(),
                data: core::ptr::null(),
                _marker: PhantomData,
            },
            // SAFETY: `Some` ↔ a valid allocation with initialised header/elements.
            |ptr| unsafe {
                GappedPresentIter {
                    occ_bits: ptr.as_ref().occupancy.bits(),
                    live_bits: ptr.as_ref().live.bits(),
                    data: data_ptr(ptr).cast_const(),
                    _marker: PhantomData,
                }
            },
        )
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
    // Dropping a `!needs_drop` element is a no-op, so skip the per-element
    // walk entirely. The manual bit-loop below is not elidable by the
    // optimiser the way slice drop glue is, so without this guard a POD
    // payload pays an O(count) walk on every drop.
    if !core::mem::needs_drop::<T>() {
        return;
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
                unsafe {
                    dealloc(
                        self.ptr.as_ptr().cast(),
                        alloc_layout::<Inner<A, T>, T>(self.cap),
                    );
                };
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

/// Drop guard for a sparse clone fill. Unlike `packed.rs`'s contiguous-prefix
/// guard, the fill writes scattered physical slots, so on unwind it must drop
/// exactly the slots set in `initialized` (not a prefix) — dropping a prefix
/// would hit uninitialised holes (UB).
struct GappedFillGuard<A: Arity, T> {
    inner: NonNull<Inner<A, T>>,
    initialized: A::Bitmap,
    cap: usize,
}

impl<A: Arity, T> Drop for GappedFillGuard<A, T> {
    fn drop(&mut self) {
        // SAFETY: `inner` is a live allocation from `alloc_layout(cap)`; the
        // slots set in `initialized` are the only initialised elements.
        unsafe {
            let dp = data_ptr(self.inner);
            for i in self.initialized.bits() {
                core::ptr::drop_in_place(dp.add(i.as_usize()));
            }
            dealloc(
                self.inner.as_ptr().cast(),
                alloc_layout::<Inner<A, T>, T>(self.cap),
            );
        }
    }
}

impl<T: Clone, A: Arity> Clone for GappedArray<T, A> {
    fn clone(&self) -> Self {
        let Some(ptr) = self.0 else {
            return Self::new();
        };
        // SAFETY: `ptr` valid per the invariant.
        let occ = unsafe { ptr.as_ref().occupancy };
        // SAFETY: `ptr` valid per the invariant.
        let live = unsafe { ptr.as_ref().live };
        let cap = self.capacity();
        let cap_exp = cap_exp_of(cap);
        // SAFETY: `cap > 0`; the guarded fill below initialises exactly the
        // `live` slots (or the guard frees them on unwind).
        let new_inner = unsafe { alloc_block::<A, T>(occ, live, cap_exp, cap) };
        // SAFETY: `ptr` is the live source block; `new_inner` is the freshly
        // allocated destination; both are valid for `data_ptr`.
        let src = unsafe { data_ptr(ptr).cast_const() };
        // SAFETY: `new_inner` is a live allocation from `alloc_block`.
        let dst = unsafe { data_ptr(new_inner) };
        let mut guard = GappedFillGuard::<A, T> {
            inner: new_inner,
            initialized: A::Bitmap::ZERO,
            cap,
        };
        for i in live.bits() {
            let p = i.as_usize();
            // SAFETY: `p` is a set live slot in `src` (initialised); `dst.add(p)`
            // is the matching uninitialised slot; `write` initialises it. On a
            // `T::clone` panic the guard drops exactly `initialized`.
            unsafe { dst.add(p).write((*src.add(p)).clone()) };
            guard.initialized = guard.initialized.with_bit(i);
        }
        core::mem::forget(guard);
        Self(Some(new_inner), PhantomData)
    }
}

/// Clones each present element of a `&FixedArray<Option<T>, A>` into a spread
/// gapped block.
impl<T: Clone, A: Arity> From<&FixedArray<Option<T>, A>> for GappedArray<T, A> {
    fn from(src: &FixedArray<Option<T>, A>) -> Self {
        let mut occupancy = A::Bitmap::ZERO;
        for (i, slot) in src {
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
        let mut live = A::Bitmap::ZERO;
        for r in 0..count {
            let p = spread_pos(r, count, cap);
            let p_idx = <A::Index as Niche>::try_from_usize(p).expect("p < cap <= N");
            live = live.with_bit(p_idx);
        }
        // SAFETY: `cap > 0`; the guarded fill initialises exactly the `live`
        // slots (or the guard cleans up on a `T::clone` panic).
        let inner = unsafe { alloc_block::<A, T>(occupancy, live, cap_exp, cap) };
        // SAFETY: `inner` is a live allocation from `alloc_block` above.
        let dp = unsafe { data_ptr(inner) };
        let mut guard = GappedFillGuard::<A, T> {
            inner,
            initialized: A::Bitmap::ZERO,
            cap,
        };
        for (r, (_i, v)) in src.iter_present().enumerate() {
            let p = spread_pos(r, count, cap);
            let p_idx = <A::Index as Niche>::try_from_usize(p).expect("p < cap <= N");
            // SAFETY: `p` is a fresh `live` slot; `write` initialises it.
            unsafe { dp.add(p).write(v.clone()) };
            guard.initialized = guard.initialized.with_bit(p_idx);
        }
        core::mem::forget(guard);
        Self(Some(inner), PhantomData)
    }
}

/// Moves each present element back into a `FixedArray<Option<T>, A>`.
impl<T, A: Arity> From<GappedArray<T, A>> for FixedArray<Option<T>, A> {
    fn from(src: GappedArray<T, A>) -> Self {
        let mut out = Self::new();
        let src = core::mem::ManuallyDrop::new(src);
        if let Some(ptr) = src.0 {
            // SAFETY: `ptr` valid per the invariant; header is initialised.
            let occ = unsafe { ptr.as_ref().occupancy };
            // SAFETY: `ptr` valid per the invariant; header is initialised.
            let live = unsafe { ptr.as_ref().live };
            // SAFETY: `ptr` valid per the invariant; header is initialised.
            let cap = 1usize << unsafe { ptr.as_ref().cap_exp };
            // SAFETY: `ptr` is a live allocation from `alloc_layout(cap)`.
            let dp = unsafe { data_ptr(ptr) };
            // The r-th occupancy index pairs with the r-th live slot.
            for (logical, physical) in occ.bits().zip(live.bits()) {
                // SAFETY: `physical` is a set live slot (initialised); `read`
                // moves it out once. `ManuallyDrop` prevents a double free.
                let v = unsafe { dp.add(physical.as_usize()).read() };
                out[logical] = Some(v);
            }
            // SAFETY: elements moved out; sole deallocation.
            unsafe { dealloc(ptr.as_ptr().cast(), alloc_layout::<Inner<A, T>, T>(cap)) };
        }
        out
    }
}

/// Clones each present element into a `FixedArray<Option<T>, A>`.
impl<T: Clone, A: Arity> From<&GappedArray<T, A>> for FixedArray<Option<T>, A> {
    fn from(src: &GappedArray<T, A>) -> Self {
        let mut out = Self::new();
        for (i, v) in src.iter_present() {
            out[i] = Some(v.clone());
        }
        out
    }
}

/// Adopts a `PackedArray` into a gapped block (capacity = next power of two ≥
/// count, spread). Moves elements (no `T: Clone`).
impl<T, A: Arity> From<PackedArray<T, A>> for GappedArray<T, A> {
    fn from(src: PackedArray<T, A>) -> Self {
        // PackedArray -> FixedArray moves elements out; FixedArray -> Gapped
        // moves them into the spread layout. Two moves, no clone.
        let fixed: FixedArray<Option<T>, A> = src.into();
        fixed.into()
    }
}

/// Clones a `&PackedArray` into a gapped block.
impl<T: Clone, A: Arity> From<&PackedArray<T, A>> for GappedArray<T, A> {
    fn from(src: &PackedArray<T, A>) -> Self {
        // Clone once into FixedArray, then move into the spread layout.
        let fixed: FixedArray<Option<T>, A> = src.into();
        fixed.into()
    }
}

/// Compacts a `GappedArray` into an exactly-sized `PackedArray` (the exact-fit
/// escape hatch). Moves elements (no `T: Clone`).
impl<T, A: Arity> From<GappedArray<T, A>> for PackedArray<T, A> {
    fn from(src: GappedArray<T, A>) -> Self {
        let fixed: FixedArray<Option<T>, A> = src.into();
        fixed.into()
    }
}

/// Clones a `&GappedArray` into an exactly-sized `PackedArray`.
impl<T: Clone, A: Arity> From<&GappedArray<T, A>> for PackedArray<T, A> {
    fn from(src: &GappedArray<T, A>) -> Self {
        // Clone once into FixedArray, then move into the exact-fit packed block.
        let fixed: FixedArray<Option<T>, A> = src.into();
        fixed.into()
    }
}

/// Iterator over present elements of a [`GappedArray`]. See
/// [`GappedArray::iter_present`].
///
/// The occupancy cursor supplies the logical index; the live cursor supplies
/// the physical slot. The two are advanced in lockstep, so the `r`-th of one
/// pairs with the `r`-th of the other.
pub struct GappedPresentIter<'a, T, A: Arity> {
    occ_bits: arity_bitmap::BitIter<A::Bitmap>,
    live_bits: arity_bitmap::BitIter<A::Bitmap>,
    data: *const T,
    _marker: PhantomData<&'a T>,
}

impl<'a, T, A: Arity> Iterator for GappedPresentIter<'a, T, A> {
    type Item = (A::Index, &'a T);
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.occ_bits.next()?;
        let p = self
            .live_bits
            .next()
            .expect("live and occupancy have equal count")
            .as_usize();
        // SAFETY: `p` is a set live bit (physical slot < cap) with an
        // initialised element; the reference is bounded by `'a` (the
        // originating `&'a GappedArray` borrow, carried via `PhantomData<&'a T>`).
        Some((i, unsafe { &*self.data.add(p) }))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.occ_bits.size_hint()
    }
}

impl<T, A: Arity> DoubleEndedIterator for GappedPresentIter<'_, T, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let i = self.occ_bits.next_back()?;
        let p = self.live_bits.next_back().expect("equal count").as_usize();
        // SAFETY: as in `next`.
        Some((i, unsafe { &*self.data.add(p) }))
    }
}

impl<T, A: Arity> ExactSizeIterator for GappedPresentIter<'_, T, A> {
    fn len(&self) -> usize {
        self.occ_bits.len()
    }
}

impl<T, A: Arity> core::iter::FusedIterator for GappedPresentIter<'_, T, A> {}

/// Iterator over all slots of a [`GappedArray`]. See [`GappedArray::iter`].
///
/// Drives off the index range (`slots`) and an `occupancy` snapshot, pulling
/// the element for a present slot from the front or back of the present-element
/// stream as the range crosses it — mirroring `PackedAllIter`. Each step is a
/// range advance plus, for a present slot, one `present` advance: `O(1)`, no
/// per-slot `select`. Because `slots` partitions the index domain between the
/// two ends and `present` holds exactly the present elements in order, the
/// front and back draws never cross.
pub struct GappedAllIter<'a, T, A: Arity> {
    present: GappedPresentIter<'a, T, A>,
    bitmap: A::Bitmap,
    slots: arity_index::NicheRangeInclusive<A::Index>,
}

impl<'a, T, A: Arity> Iterator for GappedAllIter<'a, T, A> {
    type Item = (A::Index, Option<&'a T>);
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.slots.next()?;
        if self.bitmap.test(i) {
            let (_, v) = self
                .present
                .next()
                .expect("a set occupancy bit has a matching present element");
            Some((i, Some(v)))
        } else {
            Some((i, None))
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.slots.size_hint()
    }
}

impl<T, A: Arity> DoubleEndedIterator for GappedAllIter<'_, T, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let i = self.slots.next_back()?;
        if self.bitmap.test(i) {
            let (_, v) = self
                .present
                .next_back()
                .expect("a set occupancy bit has a matching present element");
            Some((i, Some(v)))
        } else {
            Some((i, None))
        }
    }
}

impl<T, A: Arity> ExactSizeIterator for GappedAllIter<'_, T, A> {
    fn len(&self) -> usize {
        self.slots.len()
    }
}

impl<T, A: Arity> core::iter::FusedIterator for GappedAllIter<'_, T, A> {}

impl<'a, T, A: Arity> IntoIterator for &'a GappedArray<T, A> {
    type Item = (A::Index, Option<&'a T>);
    type IntoIter = GappedAllIter<'a, T, A>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl_dense_common!(
    GappedArray,
    GappedPresentIter,
    "bit cursors iterate primitive bitmaps; clippy cannot see the assoc-type bound"
);

impl_logical_serde!(GappedArray, "GappedArray");

#[cfg(test)]
mod tests {
    extern crate std;
    use proptest::prelude::*;

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
    fn get_and_get_mut_present_only() {
        let mut src = FixedArray::<Option<u16>, Arity16>::new();
        src[U4::new_masked(1)] = Some(10);
        src[U4::new_masked(8)] = Some(80);
        src[U4::new_masked(15)] = Some(150);
        let mut g = GappedArray::from(src);
        assert_eq!(g.get(U4::new_masked(1)), Some(&10));
        assert_eq!(g.get(U4::new_masked(8)), Some(&80));
        assert_eq!(g.get(U4::new_masked(15)), Some(&150));
        assert_eq!(g.get(U4::new_masked(0)), None);
        if let Some(v) = g.get_mut(U4::new_masked(8)) {
            *v = 88;
        }
        assert_eq!(g.get(U4::new_masked(8)), Some(&88));
        assert!(g.get_mut(U4::new_masked(2)).is_none());
    }

    #[test]
    fn iter_present_ascending_and_double_ended() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        for s in [1u8, 4, 14] {
            src[U4::new_masked(s)] = Some(s);
        }
        let g = GappedArray::from(src);
        let fwd: std::vec::Vec<(u8, u8)> = g.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(fwd, std::vec![(1, 1), (4, 4), (14, 14)]);
        let mut it = g.iter_present();
        assert_eq!(it.len(), 3);
        assert_eq!(it.next().map(|(i, &v)| (i.as_u8(), v)), Some((1, 1)));
        assert_eq!(it.next_back().map(|(i, &v)| (i.as_u8(), v)), Some((14, 14)));
        assert_eq!(it.next().map(|(i, &v)| (i.as_u8(), v)), Some((4, 4)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn iter_all_slots_and_into_iter() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        for s in [1u8, 5, 14] {
            src[U4::new_masked(s)] = Some(s * 10);
        }
        let g = GappedArray::from(src);
        // Forward-only must surface every present element (regression guard: an
        // earlier design lost trailing present elements on forward-only traversal).
        let fwd: std::vec::Vec<(u8, Option<u8>)> = (&g)
            .into_iter()
            .map(|(i, o)| (i.as_u8(), o.copied()))
            .collect();
        assert_eq!(fwd.len(), 16);
        assert_eq!(fwd[0], (0, None));
        assert_eq!(fwd[1], (1, Some(10)));
        assert_eq!(fwd[5], (5, Some(50)));
        assert_eq!(fwd[14], (14, Some(140)));
        // Backward-only must surface the same.
        let mut bwd: std::vec::Vec<(u8, Option<u8>)> = g
            .iter()
            .rev()
            .map(|(i, o)| (i.as_u8(), o.copied()))
            .collect();
        bwd.reverse();
        assert_eq!(bwd, fwd);
        // Interleaved double-ended visits every slot exactly once.
        let mut it = g.iter();
        let mut got: std::vec::Vec<(u8, Option<u8>)> = std::vec::Vec::new();
        let mut front = true;
        while let Some((i, o)) = if front { it.next() } else { it.next_back() } {
            got.push((i.as_u8(), o.copied()));
            front = !front;
        }
        got.sort_by_key(|(i, _)| *i);
        assert_eq!(got.len(), 16);
        assert_eq!(got[5], (5, Some(50)));
        assert_eq!(got[14], (14, Some(140)));
    }

    #[test]
    fn remove_is_move_free_and_retains_capacity() {
        let mut src = FixedArray::<Option<u16>, Arity16>::new();
        for s in [1u8, 5, 9] {
            src[U4::new_masked(s)] = Some(u16::from(s) * 10);
        }
        let mut g = GappedArray::from(src);
        let cap_before = g.capacity();
        // Capture the physical slot of slot 9 via its address; removing slot 5 must
        // not move slot 9 (its &T address is unchanged).
        let addr9 =
            core::ptr::from_ref::<u16>(g.get(U4::new_masked(9)).expect("slot 9 present")) as usize;
        assert_eq!(g.remove(U4::new_masked(5)), Some(50));
        assert_eq!(g.remove(U4::new_masked(5)), None); // absent
        assert_eq!(g.count(), 2);
        assert_eq!(g.get(U4::new_masked(1)), Some(&10));
        assert_eq!(g.get(U4::new_masked(9)), Some(&90));
        let addr9_after =
            core::ptr::from_ref::<u16>(g.get(U4::new_masked(9)).expect("slot 9 still present"))
                as usize;
        assert_eq!(addr9, addr9_after, "delete must not move other elements");
        // Removing all keeps the allocation (no auto-shrink).
        assert_eq!(g.remove(U4::new_masked(1)), Some(10));
        assert_eq!(g.remove(U4::new_masked(9)), Some(90));
        assert!(g.is_empty());
        assert_eq!(g.capacity(), cap_before);
    }

    #[test]
    fn insert_empty_overwrite_and_grow() {
        let mut g = GappedArray::<u16, Arity16>::new();
        assert_eq!(g.insert(U4::new_masked(7), 70), None); // empty -> cap 1
        assert_eq!(g.capacity(), 1);
        assert_eq!(g.get(U4::new_masked(7)), Some(&70));
        assert_eq!(g.insert(U4::new_masked(7), 77), Some(70)); // overwrite in place
        assert_eq!(g.capacity(), 1);
        // Out-of-order inserts keep logical order and grow by powers of two.
        assert_eq!(g.insert(U4::new_masked(2), 20), None);
        assert_eq!(g.insert(U4::new_masked(9), 90), None);
        assert_eq!(g.insert(U4::new_masked(0), 0), None);
        assert_eq!(g.count(), 4);
        assert!(g.capacity() >= 4 && g.capacity().is_power_of_two());
        let present: std::vec::Vec<(u8, u16)> =
            g.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(present, std::vec![(0, 0), (2, 20), (7, 77), (9, 90)]);
    }

    #[test]
    fn insert_matches_btreemap_small() {
        use std::collections::BTreeMap;
        let mut g = GappedArray::<u16, Arity16>::new();
        let mut oracle: BTreeMap<u8, u16> = BTreeMap::new();
        for (slot, val) in [(3u8, 30u16), (1, 10), (3, 33), (14, 140), (8, 80), (1, 11)] {
            let i = U4::new_masked(slot);
            assert_eq!(g.insert(i, val), oracle.insert(slot, val));
            assert_eq!(g.count(), oracle.len());
            for s in 0..16u8 {
                assert_eq!(g.get(U4::new_masked(s)).copied(), oracle.get(&s).copied());
            }
        }
    }

    #[test]
    fn insert_shifts_to_nearest_hole_without_full_respread() {
        use std::collections::BTreeMap;
        // Build a small dense run with a single trailing hole, then insert in the
        // middle: the element should shift toward the near hole, not respread.
        // Slots 0,1,2 present at cap 4 (one hole at the spread gap). Insert slot 3
        // (logical back) lands directly in a hole — no move. Then a middle insert
        // shifts only the minimal run.
        let mut src = FixedArray::<Option<u16>, Arity16>::new();
        for s in [0u8, 1, 2] {
            src[U4::new_masked(s)] = Some(u16::from(s));
        }
        let mut g = GappedArray::from(src); // cap 4, count 3
        assert_eq!(g.capacity(), 4);
        // Correctness under a mixed sequence (oracle).
        let mut oracle: BTreeMap<u8, u16> = BTreeMap::new();
        for s in [0u8, 1, 2] {
            oracle.insert(s, u16::from(s));
        }
        for (slot, val) in [(3u8, 30u16), (5, 50), (4, 40), (6, 60), (7, 70)] {
            let i = U4::new_masked(slot);
            assert_eq!(g.insert(i, val), oracle.insert(slot, val));
            assert_eq!(g.count(), oracle.len());
            for s in 0..16u8 {
                assert_eq!(g.get(U4::new_masked(s)).copied(), oracle.get(&s).copied());
            }
        }
    }

    #[test]
    fn clone_is_independent_and_preserves_layout() {
        let mut src = FixedArray::<Option<u16>, Arity16>::new();
        for s in [1u8, 4, 9] {
            src[U4::new_masked(s)] = Some(u16::from(s));
        }
        let a = GappedArray::from(src);
        let b = a.clone();
        assert_eq!(b.count(), 3);
        assert_eq!(b.capacity(), a.capacity());
        for s in 0..16u8 {
            assert_eq!(
                a.get(U4::new_masked(s)).copied(),
                b.get(U4::new_masked(s)).copied()
            );
        }
    }

    #[test]
    fn clone_panic_frees_partial() {
        use std::panic;
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        struct Panicky {
            drops: Arc<AtomicUsize>,
            clones: Arc<AtomicUsize>,
        }
        impl Clone for Panicky {
            fn clone(&self) -> Self {
                assert!(self.clones.fetch_add(1, Ordering::SeqCst) < 2, "boom");
                Self {
                    drops: self.drops.clone(),
                    clones: self.clones.clone(),
                }
            }
        }
        impl Drop for Panicky {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let clones = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Panicky>, Arity16>::new();
        for i in 0..4u8 {
            src[U4::new_masked(i)] = Some(Panicky {
                drops: drops.clone(),
                clones: clones.clone(),
            });
        }
        let g = GappedArray::from(src);
        let r = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            drop(g.clone());
        }));
        assert!(r.is_err());
        assert_eq!(drops.load(Ordering::SeqCst), 2); // the 2 cloned elements freed
        drop(g);
        assert_eq!(drops.load(Ordering::SeqCst), 6);
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
        #[test]
        fn clone_panic_frees_partial_irregular(
            // Build an irregular layout, then panic partway through cloning it.
            // Insert weighted heavy so `count >= 1` is the common case.
            build in proptest::collection::vec(
                prop_oneof![
                    5 => (0u8..16, any::<u16>()).prop_map(|(s, v)| (0u8, s, v)),  // insert
                    2 => (0u8..16).prop_map(|s| (1u8, s, 0u16)),                  // remove
                    2 => (0u8..16).prop_map(|n| (2u8, n, 0u16)),                  // reserve n
                    1 => Just((3u8, 0u8, 0u16)),                                  // shrink_to_fit
                ],
                0..64,
            ),
            boom_sel in any::<u8>(),
        ) {
            use std::sync::Arc;
            use std::sync::atomic::{AtomicUsize, Ordering};

            struct Bomb {
                drops: Arc<AtomicUsize>,
                clones: Arc<AtomicUsize>,
                boom_at: Arc<AtomicUsize>,
            }
            impl Clone for Bomb {
                fn clone(&self) -> Self {
                    // The n-th clone (0-indexed) whose index equals `boom_at` panics.
                    let n = self.clones.fetch_add(1, Ordering::SeqCst);
                    assert!(n != self.boom_at.load(Ordering::SeqCst), "boom");
                    Self { drops: self.drops.clone(), clones: self.clones.clone(), boom_at: self.boom_at.clone() }
                }
            }
            impl Drop for Bomb {
                fn drop(&mut self) { self.drops.fetch_add(1, Ordering::SeqCst); }
            }

            let drops = Arc::new(AtomicUsize::new(0));
            let clones = Arc::new(AtomicUsize::new(0));
            let boom_at = Arc::new(AtomicUsize::new(usize::MAX)); // no panic during build

            // Build an irregular `live` bitmap. Inserts/removes move Bombs (no clone);
            // reserve/shrink relocate bitwise; reserve/insert/remove/shrink permutations
            // produce holes (capacity > count) and non-spread layouts.
            let mut g: GappedArray<Bomb, Arity16> = GappedArray::new();
            for (tag, s, _v) in &build {
                match tag {
                    0 => { g.insert(U4::new_masked(*s), Bomb { drops: drops.clone(), clones: clones.clone(), boom_at: boom_at.clone() }); }
                    1 => { g.remove(U4::new_masked(*s)); } // removed Bomb drops here
                    2 => g.reserve(*s as usize),
                    _ => g.shrink_to_fit(),
                }
            }
            let count = g.count();
            prop_assume!(count >= 1); // need >= 1 element to clone-panic

            // Isolate the clone-phase accounting (build did some drops via remove/overwrite).
            drops.store(0, Ordering::SeqCst);
            clones.store(0, Ordering::SeqCst);
            // Map an arbitrary byte to a valid clone index in `0..count`; modulo
            // makes it exercise both boom == 0 (panic-on-first) and boom == count-1.
            let boom = (boom_sel as usize) % count;
            boom_at.store(boom, Ordering::SeqCst);

            // Clone panics on the (boom+1)-th element; GappedFillGuard must drop exactly
            // the `boom` already-cloned elements as the clone unwinds.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { drop(g.clone()); }));
            prop_assert!(r.is_err());
            prop_assert_eq!(drops.load(Ordering::SeqCst), boom); // freed exactly the cloned ones, no leak/double-drop

            // Dropping the original drops its `count` elements.
            drop(g);
            prop_assert_eq!(drops.load(Ordering::SeqCst), boom + count);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
        #[test]
        fn clear_drops_all_over_irregular_layout(
            build in proptest::collection::vec(
                prop_oneof![
                    5 => (0u8..16, any::<u16>()).prop_map(|(s, v)| (0u8, s, v)),
                    2 => (0u8..16).prop_map(|s| (1u8, s, 0u16)),
                    2 => (0u8..16).prop_map(|n| (2u8, n, 0u16)),
                    1 => Just((3u8, 0u8, 0u16)),
                ],
                0..64,
            ),
        ) {
            use std::sync::Arc;
            use std::sync::atomic::{AtomicUsize, Ordering};

            struct Bomb(Arc<AtomicUsize>);
            impl Drop for Bomb {
                fn drop(&mut self) { self.0.fetch_add(1, Ordering::SeqCst); }
            }

            let drops = Arc::new(AtomicUsize::new(0));
            let mut g: GappedArray<Bomb, Arity16> = GappedArray::new();
            for (tag, s, _v) in &build {
                match tag {
                    0 => { g.insert(U4::new_masked(*s), Bomb(drops.clone())); }
                    1 => { g.remove(U4::new_masked(*s)); }   // returned Bomb drops here
                    2 => g.reserve(*s as usize),
                    _ => g.shrink_to_fit(),
                }
            }
            let live = g.count();
            let cap = g.capacity();
            drops.store(0, Ordering::SeqCst); // count only clear()'s drops

            g.clear();
            // clear() drops exactly the live elements, once each (drop loop runs because
            // Bomb: Drop), and retains the allocation.
            prop_assert_eq!(drops.load(Ordering::SeqCst), live);
            prop_assert!(g.is_empty());
            prop_assert_eq!(g.capacity(), cap);

            // Dropping the (now empty) array adds no further drops.
            drop(g);
            prop_assert_eq!(drops.load(Ordering::SeqCst), live);
        }
    }

    #[test]
    fn capacity_api() {
        // with_capacity(0) is unallocated; rounds up otherwise; caps at N.
        assert_eq!(GappedArray::<u16, Arity16>::with_capacity(0).capacity(), 0);
        assert_eq!(GappedArray::<u16, Arity16>::with_capacity(5).capacity(), 8);
        assert_eq!(
            GappedArray::<u16, Arity16>::with_capacity(99).capacity(),
            16
        );
        let mut g = GappedArray::<u16, Arity16>::with_capacity(2);
        assert_eq!(g.capacity(), 2);
        assert!(g.is_empty());
        g.insert(U4::new_masked(3), 30);
        g.reserve(10); // ensure capacity >= count + 10, pow2, capped at N
        assert!(g.capacity() >= 11 && g.capacity() <= 16 && g.capacity().is_power_of_two());
        g.reserve(usize::MAX); // saturates at N, no overflow
        assert_eq!(g.capacity(), 16);
        assert_eq!(g.get(U4::new_masked(3)), Some(&30));
        // shrink_to_fit -> smallest pow2 >= count
        g.shrink_to_fit();
        assert_eq!(g.capacity(), 1);
        // clear retains the allocation
        g.clear();
        assert!(g.is_empty());
        assert_eq!(g.capacity(), 1);
        // shrink_to_fit on empty -> unallocated
        g.shrink_to_fit();
        assert_eq!(g.capacity(), 0);
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

    #[test]
    fn eq_hash_debug_are_logical() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hash;
        use std::hash::Hasher;
        fn assert_send_sync<T: Send + Sync>() {}

        let mut s1 = FixedArray::<Option<u16>, Arity16>::new();
        s1[U4::new_masked(2)] = Some(20);
        s1[U4::new_masked(9)] = Some(90);
        // Same logical content but built so capacities/holes differ.
        let mut a = GappedArray::from(s1);
        let mut b = GappedArray::<u16, Arity16>::with_capacity(16);
        b.insert(U4::new_masked(9), 90);
        b.insert(U4::new_masked(2), 20);
        assert_eq!(a, b); // gap-agnostic equality
        let h = |g: &GappedArray<u16, Arity16>| {
            let mut s = DefaultHasher::new();
            g.hash(&mut s);
            s.finish()
        };
        assert_eq!(h(&a), h(&b));
        a.remove(U4::new_masked(2));
        assert_ne!(a, b);
        let dbg = std::format!("{a:?}");
        assert!(dbg.contains("90"));
        assert_send_sync::<GappedArray<u16, Arity16>>();
    }
}
