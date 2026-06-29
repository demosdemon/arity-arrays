//! Crate-internal heap-layout helper shared by the array representations.
//!
//! Only the parts that are genuinely representation-independent live here.
//! `data_ptr` and `alloc_block` remain in each representation's module because
//! they read/write the module's own `Inner` header by field, whose shape
//! differs (`PackedArray` has one bitmap; `GappedArray` has two plus
//! `cap_exp`).

use core::alloc::Layout;

/// Returns the layout of a heap block: the header `H`, extended by an `[T; n]`
/// element array, padded to alignment. Generic over the header so both
/// representations share one definition; `H` is never inspected, only sized.
pub fn alloc_layout<H, T>(n: usize) -> Layout {
    let (layout, _) = Layout::new::<H>()
        .extend(Layout::array::<T>(n).expect("element layout overflow"))
        .expect("block layout overflow");
    layout.pad_to_align()
}
