//! [`PackedArray`]: a pointer-sized, heap-packed array storing only present
//! elements, addressed by bitmap rank-select.

use alloc::alloc::alloc;
use alloc::alloc::dealloc;
use alloc::alloc::handle_alloc_error;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;

use arity_bitmap::Bitmap;

use crate::Arity;
use crate::FixedArray;

/// Header of the heap block: the bitmap followed (after alignment padding) by
/// the element array. `#[repr(C)]` makes `data` the canonical element-array
/// address.
#[repr(C)]
struct Inner<A: Arity, T> {
    bitmap: A::Bitmap,
    /// Zero-sized address anchor for the trailing element array; obtain the
    /// base with `&raw mut (*p).data` (RFC 2582).
    data: [T; 0],
}

/// A pointer-sized, heap-packed array over arity `A`, storing only the present
/// elements.
///
/// `None` ↔ empty (no allocation). `Some(ptr)` ↔ a heap block sized to exactly
/// the present elements. The `NonNull` null-pointer niche makes this type the
/// size of a pointer for every `A`.
///
/// # Safety
///
/// Invariant upheld by every constructor and mutator: when `self.0` is
/// `Some(ptr)`, `ptr` points to a live allocation from
/// `alloc_layout::<A, T>(count)` whose `bitmap` field is initialised with
/// `bitmap != A::Bitmap::ZERO`, and whose `count == bitmap.count_ones()` element
/// slots are all initialised in ascending slot (rank) order. When `self.0` is
/// `None`, there is no allocation. The `unsafe` reads throughout this module
/// rely on this invariant.
pub struct PackedArray<T, A: Arity>(
    Option<NonNull<Inner<A, T>>>,
    PhantomData<alloc::boxed::Box<T>>,
);

// Compile-time guarantee: pointer-sized. Witnessed by whichever arity is
// enabled (the property is generic over `A`; the marker is only a witness).
#[cfg(feature = "8")]
type SizeWitness = crate::Arity8;
#[cfg(all(not(feature = "8"), feature = "16"))]
type SizeWitness = crate::Arity16;
#[cfg(all(not(feature = "8"), not(feature = "16"), feature = "32"))]
type SizeWitness = crate::Arity32;
#[cfg(all(not(feature = "8"), not(feature = "16"), not(feature = "32"), feature = "64"))]
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
    core::mem::size_of::<PackedArray<[u8; 32], SizeWitness>>()
        == core::mem::size_of::<*const ()>()
);

/// Layout of the heap block for `count` elements: `Inner` header extended by a
/// `[T; count]` array, padded to alignment.
fn alloc_layout<A: Arity, T>(count: usize) -> Layout {
    let (layout, _) = Layout::new::<Inner<A, T>>()
        .extend(Layout::array::<T>(count).expect("element layout overflow"))
        .expect("block layout overflow");
    layout.pad_to_align()
}

/// Base address of the element array within an `Inner` allocation.
///
/// # Safety
/// `inner` must point to a live allocation from `alloc_layout::<A, T>(count)`
/// with the `bitmap` field initialised.
unsafe fn data_ptr<A: Arity, T>(inner: NonNull<Inner<A, T>>) -> *mut T {
    // SAFETY: `inner` is valid per the precondition; `#[repr(C)]` places `data`
    // at the correct offset, so `&raw mut (*p).data` cast to `*mut T` is the
    // element-array base.
    unsafe { (&raw mut (*inner.as_ptr()).data).cast::<T>() }
}

/// Allocates a heap block for `count` elements and writes the header `bitmap`,
/// leaving the `count` element slots uninitialised. Returns the base `Inner`.
///
/// This is the single definition of the layout/header protocol shared by the
/// three constructors (`From<FixedArray>`, `From<&FixedArray>`, `Clone`).
///
/// # Safety
/// `count` must be `> 0` (so the layout is non-zero-sized) and equal to
/// `bitmap.count_ones()`. The caller must initialise all `count` element slots
/// before any read, and owns the allocation thereafter (dropping the elements
/// and deallocating with `alloc_layout::<A, T>(count)`).
unsafe fn alloc_block<A: Arity, T>(bitmap: A::Bitmap, count: usize) -> NonNull<Inner<A, T>> {
    let layout = alloc_layout::<A, T>(count);
    // SAFETY: `count > 0` so `layout.size() > 0`; `alloc` returns null on
    // failure, handled below.
    let Some(raw) = NonNull::new(unsafe { alloc(layout) }) else {
        handle_alloc_error(layout)
    };
    let inner = raw.cast::<Inner<A, T>>();
    // SAFETY: `inner` is freshly allocated and sized for `Inner<A, T>`; writing
    // the bitmap initialises the header before any element.
    unsafe { (&raw mut (*inner.as_ptr()).bitmap).write(bitmap) };
    inner
}

/// Drop guard for the fill phase of a block allocated by [`alloc_block`]. On
/// unwind it drops the `initialized` leading elements and frees the block;
/// callers `core::mem::forget` it once the fill completes.
struct FillGuard<A: Arity, T> {
    inner: NonNull<Inner<A, T>>,
    initialized: usize,
    capacity: usize,
}

impl<A: Arity, T> Drop for FillGuard<A, T> {
    fn drop(&mut self) {
        // SAFETY: `inner` is a live allocation from `alloc_layout::<A, T>(capacity)`;
        // its `initialized` leading elements are initialised.
        unsafe {
            let dp = data_ptr(self.inner);
            core::ptr::drop_in_place(core::ptr::slice_from_raw_parts_mut(dp, self.initialized));
            dealloc(
                self.inner.as_ptr().cast(),
                alloc_layout::<A, T>(self.capacity),
            );
        }
    }
}

impl<T, A: Arity> PackedArray<T, A> {
    /// Creates an empty `PackedArray` (no allocation).
    #[must_use]
    pub const fn new() -> Self {
        Self(None, PhantomData)
    }

    /// Returns the bitmap of present slots (`A::Bitmap::ZERO` when empty).
    #[must_use]
    pub const fn bitmap(&self) -> A::Bitmap {
        match self.0 {
            None => A::Bitmap::ZERO,
            // SAFETY: `Some` ↔ a live allocation with an initialised bitmap.
            Some(p) => unsafe { p.as_ref().bitmap },
        }
    }

    /// Returns the number of present elements.
    #[must_use]
    pub fn count(&self) -> usize {
        self.bitmap().count_ones() as usize
    }

    /// Returns `true` if there are no elements.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// Returns a reference to the element at `index`, or `None` if absent.
    #[must_use]
    pub fn get(&self, index: A::Index) -> Option<&T> {
        let ptr = self.0?;
        // SAFETY: `ptr` is valid per the invariant.
        let bm = unsafe { ptr.as_ref().bitmap };
        if !bm.test(index) {
            return None;
        }
        let rank = bm.rank(index) as usize;
        // SAFETY: `index` is present, so `rank < count`; `data_ptr` is valid and
        // `.add(rank)` is within the allocation, pointing at an initialised `T`.
        Some(unsafe { &*data_ptr(ptr).add(rank) })
    }

    /// Returns the element at dense storage position `rank`, skipping the
    /// bitmap `test`/`rank` that [`get`](Self::get) performs. Lets
    /// [`PackedAllIter`] reuse a running rank counter instead of re-scanning
    /// the bitmap per slot.
    ///
    /// # Safety
    /// The array must be non-empty and `rank` must be `< self.count()`.
    unsafe fn elem_at_rank(&self, rank: usize) -> &T {
        // SAFETY: the array is non-empty per the precondition, so `self.0` is
        // `Some`; the pointer is valid per the type invariant.
        let ptr = unsafe { self.0.unwrap_unchecked() };
        // SAFETY: `rank < count` per the precondition; `data_ptr(ptr).add(rank)`
        // is an initialised element within the allocation.
        unsafe { &*data_ptr(ptr).add(rank) }
    }

    /// Iterates over present elements as `(A::Index, &T)`, ascending.
    /// Double-ended.
    #[must_use]
    pub fn iter_present(&self) -> PackedPresentIter<'_, T, A> {
        self.0.map_or_else(
            || PackedPresentIter {
                bits: A::Bitmap::ZERO.bits(),
                bitmap: A::Bitmap::ZERO,
                data: core::ptr::null(),
                _marker: PhantomData,
            },
            // SAFETY: `Some` ↔ a valid allocation with initialised bitmap/elements.
            |ptr| unsafe {
                let bitmap = ptr.as_ref().bitmap;
                PackedPresentIter {
                    bits: bitmap.bits(),
                    bitmap,
                    data: data_ptr(ptr).cast_const(),
                    _marker: PhantomData,
                }
            },
        )
    }

    /// Iterates over all `A::LEN` slots as `(A::Index, Option<&T>)`, ascending.
    /// Double-ended.
    #[must_use]
    pub fn iter(&self) -> PackedAllIter<'_, T, A> {
        let bitmap = self.bitmap();
        PackedAllIter {
            array: self,
            bitmap,
            count: bitmap.count_ones() as usize,
            slots: A::Index::all(),
            front_rank: 0,
            back_consumed: 0,
        }
    }

    /// Returns a mutable reference to the element at `index`, or `None` if
    /// absent. Does not change which slots are present (no reallocation).
    pub fn get_mut(&mut self, index: A::Index) -> Option<&mut T> {
        let ptr = self.0?;
        // SAFETY: `ptr` is valid per the type invariant.
        let bm = unsafe { ptr.as_ref().bitmap };
        if !bm.test(index) {
            return None;
        }
        let rank = bm.rank(index) as usize;
        // SAFETY: `index` is present, so `rank < count`; `data_ptr(ptr).add(rank)`
        // is an initialised element within the allocation. The borrow is tied to
        // `&mut self`, which gives exclusive access for its lifetime.
        Some(unsafe { &mut *data_ptr(ptr).add(rank) })
    }
}

impl<T, A: Arity> Default for PackedArray<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Moves each `Some` element of a `FixedArray<Option<T>, A>` into a packed
/// block; `None` slots are dropped.
impl<T, A: Arity> From<FixedArray<Option<T>, A>> for PackedArray<T, A> {
    fn from(src: FixedArray<Option<T>, A>) -> Self {
        // Pass 1 (by ref): compute the bitmap.
        let mut bitmap = A::Bitmap::ZERO;
        for (i, slot) in &src {
            if slot.is_some() {
                bitmap = bitmap.with_bit(i);
            }
        }
        if bitmap.is_zero() {
            return Self::new();
        }
        let count = bitmap.count_ones() as usize;
        // SAFETY: `count == bitmap.count_ones() > 0`; the fill loop below
        // initialises all `count` slots before the value is observed.
        let inner = unsafe { alloc_block::<A, T>(bitmap, count) };
        // SAFETY: `inner` valid; `data_ptr` is the base of `count` element slots.
        let dp = unsafe { data_ptr(inner) };
        // Pass 2 (by value): move each `Some` into the next dense slot. No
        // `FillGuard` is needed here: moving an owned value out of `Some` cannot
        // panic and dropping a `None` runs no user code, so there is no
        // partial-init window to clean up on unwind.
        let mut rank = 0usize;
        for (_i, slot) in src {
            if let Some(v) = slot {
                // SAFETY: `rank < count`; `dp.add(rank)` is an uninitialised slot
                // within the allocation; `write` initialises it.
                unsafe { dp.add(rank).write(v) };
                rank += 1;
            }
        }
        Self(Some(inner), PhantomData)
    }
}

/// Clones each present element of a `&FixedArray<Option<T>, A>` into a packed
/// block.
impl<T: Clone, A: Arity> From<&FixedArray<Option<T>, A>> for PackedArray<T, A> {
    fn from(src: &FixedArray<Option<T>, A>) -> Self {
        let mut bitmap = A::Bitmap::ZERO;
        for (i, slot) in src {
            if slot.is_some() {
                bitmap = bitmap.with_bit(i);
            }
        }
        if bitmap.is_zero() {
            return Self::new();
        }
        let count = bitmap.count_ones() as usize;
        // SAFETY: `count == bitmap.count_ones() > 0`; the guarded fill loop
        // initialises all `count` slots (or the guard cleans up on unwind).
        let inner = unsafe { alloc_block::<A, T>(bitmap, count) };
        // SAFETY: `inner` valid; `data_ptr` is the base of `count` element slots.
        let dp = unsafe { data_ptr(inner) };
        // `T::clone` may panic; the guard frees already-cloned elements + the
        // block on unwind.
        let mut guard = FillGuard {
            inner,
            initialized: 0,
            capacity: count,
        };
        for (_i, v) in src.iter_present() {
            // SAFETY: at most `count` present elements; `dp.add(initialized)` is an
            // uninitialised in-bounds slot; `write` initialises it.
            unsafe { dp.add(guard.initialized).write(v.clone()) };
            guard.initialized += 1;
        }
        core::mem::forget(guard);
        Self(Some(inner), PhantomData)
    }
}

/// Moves each element of a `PackedArray` back into a `FixedArray<Option<T>, A>`
/// (no `T: Clone` bound).
impl<T, A: Arity> From<PackedArray<T, A>> for FixedArray<Option<T>, A> {
    fn from(src: PackedArray<T, A>) -> Self {
        let mut out = Self::new();
        // Prevent `PackedArray::drop` so we can move elements out, then free.
        let src = core::mem::ManuallyDrop::new(src);
        if let Some(ptr) = src.0 {
            // SAFETY: `ptr` valid per the invariant.
            let bitmap = unsafe { ptr.as_ref().bitmap };
            let count = bitmap.count_ones() as usize;
            // SAFETY: `ptr` valid; base of `count` initialised elements.
            let dp = unsafe { data_ptr(ptr) };
            for (rank, index) in bitmap.bits().enumerate() {
                // SAFETY: `rank < count`; `dp.add(rank)` is an initialised element;
                // `read` moves it out without dropping. `ManuallyDrop` prevents a
                // double free; each element is read exactly once (bits() yields
                // each set index once, ascending == storage order).
                let v = unsafe { dp.add(rank).read() };
                out[index] = Some(v);
            }
            let layout = alloc_layout::<A, T>(count);
            // SAFETY: elements moved out; sole deallocation of this block.
            unsafe { dealloc(ptr.as_ptr().cast(), layout) };
        }
        out
    }
}

/// Clones each present element of a `&PackedArray` into a
/// `FixedArray<Option<T>, A>`.
impl<T: Clone, A: Arity> From<&PackedArray<T, A>> for FixedArray<Option<T>, A> {
    fn from(src: &PackedArray<T, A>) -> Self {
        let mut out = Self::new();
        for (index, v) in src.iter_present() {
            out[index] = Some(v.clone());
        }
        out
    }
}

use arity_index::Niche;

/// Iterator over present elements of a [`PackedArray`]. See
/// [`PackedArray::iter_present`].
pub struct PackedPresentIter<'a, T, A: Arity> {
    bits: arity_bitmap::BitIter<A::Bitmap>,
    bitmap: A::Bitmap,
    data: *const T,
    _marker: PhantomData<&'a T>,
}

impl<'a, T, A: Arity> PackedPresentIter<'a, T, A> {
    fn elem(&self, index: A::Index) -> (A::Index, &'a T) {
        let rank = self.bitmap.rank(index) as usize;
        // SAFETY: `index` is a set bit of the original `bitmap` snapshot, so
        // `rank < count`; `data` is the element base; `.add(rank)` is in bounds
        // and initialised. `rank` uses the original bitmap, not the drained
        // `bits` state, so the offset is correct in either direction.
        (index, unsafe { &*self.data.add(rank) })
    }
}

impl<'a, T, A: Arity> Iterator for PackedPresentIter<'a, T, A> {
    type Item = (A::Index, &'a T);
    fn next(&mut self) -> Option<Self::Item> {
        self.bits.next().map(|i| self.elem(i))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.bits.size_hint()
    }
}

impl<T, A: Arity> DoubleEndedIterator for PackedPresentIter<'_, T, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.bits.next_back().map(|i| self.elem(i))
    }
}

impl<T, A: Arity> ExactSizeIterator for PackedPresentIter<'_, T, A> {
    fn len(&self) -> usize {
        self.bits.len()
    }
}

impl<T, A: Arity> core::iter::FusedIterator for PackedPresentIter<'_, T, A> {}

/// Iterator over all slots of a [`PackedArray`]. See [`PackedArray::iter`].
///
/// Drives off a `bitmap` snapshot and two running rank counters rather than
/// calling [`PackedArray::get`] per slot, so each step is an O(1) bit `test`
/// plus a counter bump — no repeated `rank` scan. `slots` (the index range)
/// owns termination and front/back crossing; because it partitions the indices
/// between the two ends, a present index yielded from the front has dense rank
/// `front_rank`, and one yielded from the back has dense rank
/// `count - 1 - back_consumed`.
///
/// # Safety
///
/// Invariant: `front_rank` counts the present slots yielded from the front and
/// `back_consumed` counts those yielded from the back, with
/// `front_rank + back_consumed <= count` at all times. Because `slots`
/// partitions the index domain between the two ends, no present slot is counted
/// by both, so each computed dense rank is `< count` — which the private
/// `elem_at_rank` helper requires.
pub struct PackedAllIter<'a, T, A: Arity> {
    array: &'a PackedArray<T, A>,
    bitmap: A::Bitmap,
    count: usize,
    slots: arity_index::NicheRangeInclusive<A::Index>,
    front_rank: usize,
    back_consumed: usize,
}

impl<'a, T, A: Arity> Iterator for PackedAllIter<'a, T, A> {
    type Item = (A::Index, Option<&'a T>);
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.slots.next()?;
        if self.bitmap.test(i) {
            // SAFETY: `i` is set, so the array is non-empty and
            // `front_rank == rank(i) < count`.
            let v = unsafe { self.array.elem_at_rank(self.front_rank) };
            self.front_rank += 1;
            Some((i, Some(v)))
        } else {
            Some((i, None))
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.slots.size_hint()
    }
}

impl<T, A: Arity> DoubleEndedIterator for PackedAllIter<'_, T, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let i = self.slots.next_back()?;
        if self.bitmap.test(i) {
            let rank = self.count - 1 - self.back_consumed;
            // SAFETY: `i` is set, so the array is non-empty and
            // `rank == rank(i) < count`.
            let v = unsafe { self.array.elem_at_rank(rank) };
            self.back_consumed += 1;
            Some((i, Some(v)))
        } else {
            Some((i, None))
        }
    }
}

impl<T, A: Arity> ExactSizeIterator for PackedAllIter<'_, T, A> {
    fn len(&self) -> usize {
        self.slots.len()
    }
}

impl<T, A: Arity> core::iter::FusedIterator for PackedAllIter<'_, T, A> {}

impl<'a, T, A: Arity> IntoIterator for &'a PackedArray<T, A> {
    type Item = (A::Index, Option<&'a T>);
    type IntoIter = PackedAllIter<'a, T, A>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T, A: Arity> Drop for PackedArray<T, A> {
    fn drop(&mut self) {
        let Some(ptr) = self.0 else { return };
        // SAFETY: `ptr` is valid per the invariant; the bitmap is initialised.
        let count = unsafe { ptr.as_ref().bitmap.count_ones() as usize };
        // SAFETY: `ptr` valid; `data_ptr` is the base of `count` initialised `T`.
        let dp = unsafe { data_ptr(ptr) };
        // SAFETY: `dp..dp+count` are initialised; `drop_in_place` over the slice
        // drops each exactly once.
        unsafe { core::ptr::drop_in_place(core::ptr::slice_from_raw_parts_mut(dp, count)) };
        // SAFETY: `ptr` came from `alloc(alloc_layout::<A, T>(count))`; elements
        // are dropped; this is the sole deallocation.
        unsafe { dealloc(ptr.as_ptr().cast(), alloc_layout::<A, T>(count)) };
    }
}

impl<T: Clone, A: Arity> Clone for PackedArray<T, A> {
    fn clone(&self) -> Self {
        let Some(ptr) = self.0 else {
            return Self::new();
        };
        // SAFETY: `ptr` valid per the invariant.
        let bitmap = unsafe { ptr.as_ref().bitmap };
        let count = bitmap.count_ones() as usize;
        // SAFETY: `count == bitmap.count_ones() > 0` (the source is non-empty);
        // the guarded fill loop initialises all `count` slots.
        let new_inner = unsafe { alloc_block::<A, T>(bitmap, count) };
        // SAFETY: `ptr` is valid per the invariant; `data_ptr` gives the element base.
        let src = unsafe { data_ptr(ptr).cast_const() };
        // SAFETY: `new_inner` was just allocated; `data_ptr` gives the element base.
        let dst = unsafe { data_ptr(new_inner) };

        // `T::clone` may panic; the guard frees already-cloned elements + the
        // block on unwind.
        let mut guard = FillGuard {
            inner: new_inner,
            initialized: 0,
            capacity: count,
        };
        for i in 0..count {
            // SAFETY: `i < count`; `src.add(i)` is initialised; `dst.add(i)` is an
            // uninitialised slot; `write` initialises it.
            unsafe { dst.add(i).write((*src.add(i)).clone()) };
            guard.initialized = i + 1;
        }
        core::mem::forget(guard);
        Self(Some(new_inner), PhantomData)
    }
}

impl<T: PartialEq, A: Arity> PartialEq for PackedArray<T, A> {
    fn eq(&self, other: &Self) -> bool {
        self.bitmap() == other.bitmap()
            && self
                .iter_present()
                .map(|(_, v)| v)
                .eq(other.iter_present().map(|(_, v)| v))
    }
}

impl<T: Eq, A: Arity> Eq for PackedArray<T, A> {}

impl<T: core::hash::Hash, A: Arity> core::hash::Hash for PackedArray<T, A> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.count().hash(state);
        for (i, v) in self.iter_present() {
            i.as_usize().hash(state);
            v.hash(state);
        }
    }
}

impl<T: core::fmt::Debug, A: Arity> core::fmt::Debug for PackedArray<T, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_map()
            .entries(self.iter_present().map(|(i, v)| (i.as_usize(), v)))
            .finish()
    }
}

// SAFETY: `PackedArray` exclusively owns its allocation; sending it across
// threads is sound when `T: Send`.
unsafe impl<T: Send, A: Arity> Send for PackedArray<T, A> {}
// SAFETY: `&PackedArray` yields only `&T`; no interior mutability.
unsafe impl<T: Sync, A: Arity> Sync for PackedArray<T, A> {}

// `NonNull` is `!UnwindSafe`; `PackedArray` owns its data with no shared/cyclic
// state, so these hold whenever `T` does.
impl<T: core::panic::UnwindSafe, A: Arity> core::panic::UnwindSafe for PackedArray<T, A> {}
impl<T: core::panic::RefUnwindSafe, A: Arity> core::panic::RefUnwindSafe for PackedArray<T, A> {}

// `PackedPresentIter` holds a `*const T` (which suppresses the auto-impls) but
// only ever yields `&T` — it behaves like a `slice::Iter`, so it is
// `Send`/`Sync` exactly when `T: Sync`. (`PackedAllIter` borrows
// `&PackedArray`, so it derives `Send`/`Sync` automatically once `PackedArray:
// Sync`.)
#[expect(
    clippy::non_send_fields_in_send_ty,
    reason = "`bits` iterates over `A::Bitmap`, a primitive type that is always Send; \
              clippy cannot verify the associated-type bound statically"
)]
// SAFETY: the raw pointer is used only for shared reads bounded by `&'a self`.
unsafe impl<T: Sync, A: Arity> Send for PackedPresentIter<'_, T, A> {}
// SAFETY: as above — shared, read-only access; no interior mutability.
unsafe impl<T: Sync, A: Arity> Sync for PackedPresentIter<'_, T, A> {}

#[cfg(test)]
mod tests {
    extern crate std;

    use arity_index::U4;

    use super::*;
    use crate::Arity16;
    use crate::Arity256;
    use crate::FixedArray;

    #[test]
    fn pointer_sized_and_empty() {
        assert_eq!(
            core::mem::size_of::<PackedArray<[u8; 32], Arity16>>(),
            core::mem::size_of::<*const ()>()
        );
        let p = PackedArray::<u8, Arity16>::new();
        assert_eq!(p.count(), 0);
        assert!(p.is_empty());
        assert_eq!(p.get(U4::new_masked(0)), None);
    }

    #[test]
    fn from_fixed_and_get() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(0)] = Some(10);
        src[U4::new_masked(7)] = Some(70);
        src[U4::new_masked(15)] = Some(150);
        let p = PackedArray::from(src);
        assert_eq!(p.count(), 3);
        assert_eq!(p.get(U4::new_masked(0)), Some(&10));
        assert_eq!(p.get(U4::new_masked(7)), Some(&70));
        assert_eq!(p.get(U4::new_masked(15)), Some(&150));
        assert_eq!(p.get(U4::new_masked(1)), None);
        assert_eq!(p.get(U4::new_masked(8)), None);
    }

    #[test]
    fn single_child_rank_zero_every_slot() {
        // Exercises the rank-zero boundary at every slot of every arity edge.
        for slot in 0..16u8 {
            let mut src = FixedArray::<Option<u8>, Arity16>::new();
            src[U4::new_masked(slot)] = Some(slot);
            let p = PackedArray::from(src);
            assert_eq!(p.count(), 1, "slot {slot}");
            assert_eq!(p.get(U4::new_masked(slot)), Some(&slot), "slot {slot}");
        }
    }

    #[test]
    fn arity256_boundary() {
        let mut src = FixedArray::<Option<u16>, Arity256>::new();
        src[0] = Some(1);
        src[255] = Some(2);
        let p = PackedArray::from(src);
        assert_eq!(p.count(), 2);
        assert_eq!(p.get(0), Some(&1));
        assert_eq!(p.get(255), Some(&2));
        assert_eq!(p.get(128), None);
        assert_eq!(
            core::mem::size_of::<PackedArray<u16, Arity256>>(),
            core::mem::size_of::<*const ()>()
        );
    }

    #[test]
    fn iter_present_ascending_and_double_ended() {
        extern crate alloc;
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(1)] = Some(1);
        src[U4::new_masked(4)] = Some(4);
        src[U4::new_masked(14)] = Some(14);
        let p = PackedArray::from(src);

        let fwd: alloc::vec::Vec<(u8, u8)> =
            p.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(fwd, alloc::vec![(1, 1), (4, 4), (14, 14)]);

        let mut it = p.iter_present();
        assert_eq!(it.len(), 3);
        assert_eq!(it.next().map(|(i, &v)| (i.as_u8(), v)), Some((1, 1)));
        assert_eq!(it.next_back().map(|(i, &v)| (i.as_u8(), v)), Some((14, 14)));
        assert_eq!(it.next().map(|(i, &v)| (i.as_u8(), v)), Some((4, 4)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn iter_all_slots() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(5)] = Some(5);
        let p = PackedArray::from(src);
        let all: alloc::vec::Vec<(u8, Option<u8>)> =
            p.iter().map(|(i, opt)| (i.as_u8(), opt.copied())).collect();
        assert_eq!(all.len(), 16);
        assert_eq!(all[5], (5, Some(5)));
        assert_eq!(all[0], (0, None));
        assert_eq!(all[15], (15, None));
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

        let counter = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Counted>, Arity16>::new();
        src[U4::new_masked(2)] = Some(Counted(counter.clone()));
        src[U4::new_masked(7)] = Some(Counted(counter.clone()));
        let p = PackedArray::from(src);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        drop(p);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn clone_is_independent() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;

        struct Counted(Arc<AtomicUsize>);
        impl Clone for Counted {
            fn clone(&self) -> Self {
                Self(self.0.clone())
            }
        }
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Counted>, Arity16>::new();
        src[U4::new_masked(1)] = Some(Counted(counter.clone()));
        src[U4::new_masked(9)] = Some(Counted(counter.clone()));
        let original = PackedArray::from(src);
        let cloned = original.clone();
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        drop(original);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        drop(cloned);
        assert_eq!(counter.load(Ordering::SeqCst), 4);
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
        let p = PackedArray::from(src);
        let r = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            drop(p.clone());
        }));
        assert!(r.is_err());
        // The 2 successfully-cloned elements were freed by the guard on unwind.
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        drop(p);
        assert_eq!(drops.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn owned_roundtrip_lossless() {
        extern crate alloc;
        use alloc::vec::Vec;
        for slots in [&[][..], &[0, 7, 15][..], &(0..16).collect::<Vec<u8>>()[..]] {
            let mut src = FixedArray::<Option<u8>, Arity16>::new();
            for &s in slots {
                src[U4::new_masked(s)] = Some(s);
            }
            let packed = PackedArray::from(src);
            let back: FixedArray<Option<u8>, Arity16> = packed.into();
            for s in 0..16u8 {
                let expected = slots.contains(&s).then_some(s);
                assert_eq!(*back.get(U4::new_masked(s)), expected, "slot {s}");
            }
        }
    }

    #[test]
    fn by_ref_roundtrip_lossless() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(4)] = Some(4);
        src[U4::new_masked(9)] = Some(9);
        let packed = PackedArray::<u8, Arity16>::from(&src);
        let back: FixedArray<Option<u8>, Arity16> = (&packed).into();
        for s in 0..16u8 {
            let expected = matches!(s, 4 | 9).then_some(s);
            assert_eq!(*back.get(U4::new_masked(s)), expected, "slot {s}");
        }
    }

    #[test]
    fn eq_and_debug() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(2)] = Some(2);
        let a = PackedArray::from(src);
        let b = a.clone();
        assert_eq!(a, b);
        let mut src2 = FixedArray::<Option<u8>, Arity16>::new();
        src2[U4::new_masked(3)] = Some(3);
        assert_ne!(a, PackedArray::from(src2));
        // Debug renders present slots.
        let s = std::format!("{a:?}");
        assert!(s.contains('2'));
    }

    #[test]
    fn iter_all_double_ended_interleaved() {
        // Alternating next()/next_back() must visit every slot exactly once and
        // map each present slot to its correct element via the running ranks.
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        for s in [1u8, 4, 9, 14] {
            src[U4::new_masked(s)] = Some(s * 10);
        }
        let p = PackedArray::from(src);

        let mut it = p.iter();
        let mut got: alloc::vec::Vec<(u8, Option<u8>)> = alloc::vec::Vec::new();
        let mut take_front = true;
        while let Some((i, opt)) = if take_front {
            it.next()
        } else {
            it.next_back()
        } {
            got.push((i.as_u8(), opt.copied()));
            take_front = !take_front;
        }

        assert_eq!(got.len(), 16);
        got.sort_by_key(|(i, _)| *i);
        for (i, opt) in got {
            let expected = matches!(i, 1 | 4 | 9 | 14).then_some(i * 10);
            assert_eq!(opt, expected, "slot {i}");
        }
    }

    #[test]
    fn zst_roundtrip() {
        // Zero-sized `T`: the block is sized to the bitmap alone and the element
        // writes/reads are no-ops, but rank-select and roundtrip must still hold.
        let mut src = FixedArray::<Option<()>, Arity16>::new();
        for s in [0u8, 3, 15] {
            src[U4::new_masked(s)] = Some(());
        }
        let p = PackedArray::from(src);
        assert_eq!(p.count(), 3);
        assert_eq!(p.get(U4::new_masked(0)), Some(&()));
        assert_eq!(p.get(U4::new_masked(3)), Some(&()));
        assert_eq!(p.get(U4::new_masked(15)), Some(&()));
        assert_eq!(p.get(U4::new_masked(1)), None);

        let present: alloc::vec::Vec<u8> = p.iter_present().map(|(i, &())| i.as_u8()).collect();
        assert_eq!(present, alloc::vec![0, 3, 15]);

        let back: FixedArray<Option<()>, Arity16> = p.into();
        for s in 0..16u8 {
            let expected = matches!(s, 0 | 3 | 15).then_some(());
            assert_eq!(*back.get(U4::new_masked(s)), expected, "slot {s}");
        }
    }

    #[test]
    fn get_mut_mutates_present_only() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(2)] = Some(20);
        src[U4::new_masked(9)] = Some(90);
        let mut p = PackedArray::from(src);

        // Mutate a present slot through the &mut.
        if let Some(v) = p.get_mut(U4::new_masked(9)) {
            *v = 99;
        }
        assert_eq!(p.get(U4::new_masked(9)), Some(&99));
        assert_eq!(p.get(U4::new_masked(2)), Some(&20));

        // Absent slot yields None.
        assert!(p.get_mut(U4::new_masked(5)).is_none());
        // Empty array yields None.
        let mut empty = PackedArray::<u8, Arity16>::new();
        assert!(empty.get_mut(U4::new_masked(0)).is_none());
    }
}
