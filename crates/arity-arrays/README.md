# arity-arrays

Fixed and pointer-sized heap-packed arrays over a generic arity, indexed without bounds checks.

`FixedArray<T, A>` is a full-width inline array (one slot per index); `PackedArray<T, A>` is a pointer-sized, heap-packed representation that stores only the present elements. Both are generic over the `Arity` trait, which pairs an index type with a bitmap backing and a `hybrid-array` size. Six concrete arities are provided: `Arity8`, `Arity16`, `Arity32`, `Arity64`, `Arity128`, and `Arity256`.

## Usage

```rust
use arity_arrays::{Arity16, FixedArray, PackedArray};
use arity_arrays::index::{Niche, U4};

let mut full = FixedArray::<Option<u32>, Arity16>::new();
full[U4::new_masked(1)] = Some(10);
full[U4::new_masked(9)] = Some(90);

// Pack: pointer-sized handle, two elements on the heap.
let packed = PackedArray::from(&full);
assert_eq!(packed.count(), 2);
assert_eq!(packed.get(U4::new_masked(9)), Some(&90));

let present: Vec<(u8, u32)> =
    packed.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
assert_eq!(present, vec![(1, 10), (9, 90)]);
```

## `no_std`

This crate is `#![no_std]` but requires `alloc` (heap allocation for `PackedArray`).

## MSRV

Minimum Supported Rust Version: **1.92**.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
