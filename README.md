# arity-arrays

Fixed-arity array storage indexed by bounds-check-free niche integers, with a
compact heap-packed sparse representation. Three small, `no_std` crates.

This workspace generalizes the hexary (16-ary) trie child-storage layout from
[`ava-labs/firewood#2100`](https://github.com/ava-labs/firewood/pull/2100) from a
single 16-wide layout to arbitrary power-of-two arities `N ∈ {8, 16, 32, 64, 128, 256}`.

## Three representations

A branch node stores its children in a fixed array indexed by a small integer.
Three layouts are provided:

- **Full-width** (`FixedArray<T, A>`) — one slot per index, every slot
  materialized. Cheap random access, fixed size regardless of occupancy.
- **Packed** (`PackedArray<T, A>`) — only the present entries stored, addressed by
  a bitmap via rank-select. Pointer-sized (and zero-heap) when empty; heap cost
  proportional to occupancy.
- **Gapped** (`GappedArray<T, A>`) — heap-backed with spare capacity kept at a
  geometric (power-of-two) size and gaps between elements so deletes are always
  move-free and inserts minimize moves. The write-throughput corner: trades
  memory for lower mutation cost.

The packed form is the memory-amplification mitigation from firewood#2100: a
16-slot `FixedArray<Option<[u8; 32]>>` occupies a constant **528 bytes**
regardless of how many slots are filled, while `PackedArray` costs one pointer
when empty and `bitmap + occupancy × size_of::<T>` (plus header padding) when
populated. Exact figures (handle + heap), computed by `cargo test --test
memory_report`:

### Cell A — Arity16 + [u8; 32]

| occupancy | PackedArray | GappedArray | FixedArray | Box<[Option<T>]> |
|----------:|------------:|------------:|-----------:|-----------------:|
| 0 | 8 | 8 | 528 | 544 |
| 1 | 42 | 46 | 528 | 544 |
| 4 | 138 | 142 | 528 | 544 |
| 8 | 266 | 270 | 528 | 544 |
| 16 | 522 | 526 | 528 | 544 |

All three rely on a **niche integer index** — a small type whose value is statically
known to be in `0..N`, which (a) makes `Option<Index>` one byte via niche
optimization and (b) lets the compiler elide bounds checks when indexing.

## Arity → index → bitmap

Each arity wires a niche index type to a bitmap backing of matching width:

| Arity `N` | Index type | `Option<Index>` | Bitmap backing |
| --------: | ---------- | --------------- | -------------- |
|         8 | `U3`       | 1 byte          | `u8`           |
|        16 | `U4`       | 1 byte          | `u16`          |
|        32 | `U5`       | 1 byte          | `u32`          |
|        64 | `U6`       | 1 byte          | `u64`          |
|       128 | `U7`       | 1 byte          | `u128`         |
|       256 | `u8`       | 2 bytes¹        | `U256`         |

¹ Arity-256 uses the native `u8` as its index. `u8`'s maximum (255) is already
`< 256`, so indexing a 256-element array elides the bounds check without a custom
type; `Option<u8>` is 2 bytes, but no `Option<index>` is stored on a hot path, so
this costs nothing in practice.

The wiring is a compile-time guarantee: for every arity,
`Index::COUNT == LEN == Bitmap::WIDTH`.

## Throughput benchmarks

`just bench` runs the divan throughput suite (`cargo bench -p arity-arrays
--bench throughput`; pass divan args after `--`, e.g. `just bench -- --sample-count 3`).

Current state, medians on an Apple M3 Max (MacBook Pro), the array layouts vs
the standard maps (`usize` keys) at Arity256, full occupancy:

| op (Arity256, full) | GappedArray | PackedArray | BTreeMap | HashMap | note |
| :--- | ---: | ---: | ---: | ---: | :--- |
| `get_hit` | ~13 ns | ~1.4 ns | ~5 ns | ~17 ns | flat across occupancy: one `select` per lookup is the holed layout's irreducible cost |
| `remove` | ~33 ns | ~79 ns | ~601 ns | ~39 ns | move-free delete — **Gapped wins**, flat in occupancy |
| `insert_replace` | ~32 ns | ~13 ns | ~640 ns | ~29 ns | flat, in-place |
| `insert_new`¹ | ~510 ns | ~50 ns | ~304 ns | ~29 ns | O(count): the benched fills are all capacity boundaries, so only the grow + respread path is measured (worst case, not typical insert) |
| `iter_present` | ~550 ns | ~656 ns | ~187 ns | ~140 ns | comparable |

¹ `insert_new` maxes out at fill 128 in the suite (its sample fills are all
powers of two); the other rows are at fill 256.

`GappedArray` trades memory and lookup cost for cheap mutation: at Arity16 its
build/churn workload is ~2× faster than `PackedArray`; at Arity256 the
build/resize path is ~3× slower. It is the write-throughput corner, not a
general-purpose default.

> [!NOTE]
> By-value single-op benches (`remove`, `insert_*`) drop the container inside the
> timed region, so a payload's `Drop` cost is included. Use `black_box`-returning
> workload benches to time operations in isolation.

## Crates

In dependency order:

| Crate | Purpose |
| :--- | :--- |
| [`arity-index`](crates/arity-index) | Bounds-check-free niche integer index types (`U3`–`U7`, and `u8` for arity-256) with double-ended range iterators. No `alloc`. |
| [`arity-bitmap`](crates/arity-bitmap) | Fixed-width bitmaps (`u8`–`u128`, `U256`) indexed by the niche integers, with a double-ended set-bit iterator. **No `unsafe` code.** No `alloc`. |
| [`arity-arrays`](crates/arity-arrays) | `FixedArray`, `PackedArray`, and `GappedArray` over the sealed `Arity` trait. The only crate that needs `alloc`, and the only one with `unsafe`. |

Splitting this way keeps the primitive types reusable and lets their tests run
without touching the allocator. `arity-bitmap` depends on `arity-index` so the
`Bitmap` trait speaks in the typed index rather than raw `usize` — which makes
every bit position statically `< WIDTH`, eliminating the shift-UB precondition
entirely.

## Example

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

## Cargo features

All three crates expose per-arity features `8`, `16`, `32`, `64`, `128`, `256`
(all default-on) so a consumer can compile only the widths it uses — e.g. the
hexary (firewood) shape is `default-features = false, features = ["16"]`. The
features are **additive** and safe to combine. The arrays crate additionally
offers `serde`, `serde_with` (the `Compact` adapter), an `ethnum` arity-256
backing passthrough, and `std`. See each crate's README for the full table.

> [!NOTE]
> The test suite runs only under the default (all-arity) feature set — run
> `cargo test`, not a per-arity `cargo test --no-default-features`. Feature
> gating is configuration, not logic, so the lean configurations are checked at
> the library-clippy level rather than re-run as a test matrix.

## Versioning and MSRV

These crates are at **`0.1.0`** — production-*worthy*, but reserving the right to
refine the API with real downstream use before a `1.0` stability commitment.
Under Cargo semver, `0.1.x` signals that a breaking change bumps the minor
version.

- The arity features are additive and safe to combine.
- `ethnum` is **additive within the stability guarantee**: it changes the concrete
  type of the 256-bit bitmap backing, but that type is `#[doc(hidden)]` and is not
  a stable API name (access it only through `<Arity256 as Arity>::Bitmap` or
  generically as `B: Bitmap`), so the swap has no supported type-identity
  consequence. See the `arity-bitmap` README.
- The serde wire formats (the logical form and `Compact`) are snapshot-locked but
  **not yet guaranteed stable**: they may change before `1.0` if a production
  consumer's encoding needs differ.
- **MSRV: 1.92.** MSRV bumps are documented as a minor-version event.

## License

MIT — see [LICENSE](LICENSE) or <https://opensource.org/licenses/MIT>.
