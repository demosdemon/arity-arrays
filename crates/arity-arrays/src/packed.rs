//! [`PackedArray`]: a pointer-sized, heap-packed array storing only present
//! elements, addressed by bitmap rank-select.

use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;

use alloc::alloc::{alloc, handle_alloc_error};

use arity_bitmap::Bitmap;

use crate::{Arity, FixedArray};

/// Header of the heap block: the bitmap followed (after alignment padding) by the
/// element array. `#[repr(C)]` makes `data` the canonical element-array address.
#[repr(C)]
struct Inner<A: Arity, T> {
    bitmap: A::Bitmap,
    /// Zero-sized address anchor for the trailing element array; obtain the base
    /// with `&raw mut (*p).data` (RFC 2582).
    data: [T; 0],
}

/// An immutable, pointer-sized, heap-packed array over arity `A`.
///
/// `None` ↔ empty (no allocation). `Some(ptr)` ↔ a heap block sized to exactly
/// the present elements. The `NonNull` null-pointer niche makes this type the
/// size of a pointer for every `A`.
pub struct PackedArray<T, A: Arity>(Option<NonNull<Inner<A, T>>>, PhantomData<alloc::boxed::Box<T>>);

// Compile-time guarantee: pointer-sized.
const _: () = assert!(
    core::mem::size_of::<PackedArray<[u8; 32], crate::Arity16>>()
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
}

impl<T, A: Arity> Default for PackedArray<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Moves each `Some` element of a `FixedArray<Option<T>, A>` into a packed block;
/// `None` slots are dropped.
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
        let layout = alloc_layout::<A, T>(count);
        // SAFETY: `count > 0` so `layout.size() > 0`; `alloc` returns null on
        // failure, handled below.
        let Some(raw) = NonNull::new(unsafe { alloc(layout) }) else {
            handle_alloc_error(layout)
        };
        let inner = raw.cast::<Inner<A, T>>();
        // SAFETY: `inner` is freshly allocated and sized for `Inner<A, T>`;
        // writing the bitmap initialises the header before any element.
        unsafe { (&raw mut (*inner.as_ptr()).bitmap).write(bitmap) };
        // SAFETY: `inner` valid; `data_ptr` is the base of `count` element slots.
        let dp = unsafe { data_ptr(inner) };
        // Pass 2 (by value): move each `Some` into the next dense slot.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arity16, Arity256, FixedArray};
    use arity_index::U4;

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
}
