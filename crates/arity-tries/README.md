# arity-tries

Path-compressed fixed-arity tries over [`arity-arrays`](../arity-arrays),
generic over the value type, the edge type, the arity, the children
representation, an application-provided node store, and a pluggable hash
scheme.

This crate is `#![no_std]` but requires `alloc`. It ships one node type and
the algorithms over it: lookup, `insert`, `remove`, `remove_prefix`, in-order
iteration, structural validation, and a Merkle hash walk. Every walk runs over
an explicit heap stack, so a hostile or accidental deep trie cannot overflow
the call stack, including in `Drop`.

## Usage

`InMemory` is the reference store: owning edges, reference handles, no
failures. An adopter with persistent nodes implements `EdgeStore` over its own
edge type instead.

```rust
use arity_tries::{Arity16, InMemory, Packed, Node, MemEdge, Path};
use arity_tries::{get, insert, iter, remove, remove_prefix, validate};
use arity_tries::key::nibbles;

type Edge = MemEdge<Vec<u8>, Arity16, Packed, [u8; 32]>;
type Trie = Option<Node<Vec<u8>, Edge, Arity16, Packed>>;

let mut root: Trie = None;
let mut store = InMemory::<[u8; 32]>::default();

// Keys are index paths; `nibbles` splits bytes into hexary indices.
insert(&mut root, &mut store, &nibbles(b"cat"), b"meow".to_vec()).unwrap();
insert(&mut root, &mut store, &nibbles(b"car"), b"vroom".to_vec()).unwrap();
insert(&mut root, &mut store, &nibbles(b"dog"), b"woof".to_vec()).unwrap();

assert_eq!(get(root.as_ref(), &store, &nibbles(b"car")).unwrap(), Some(b"vroom".to_vec()));
assert_eq!(get(root.as_ref(), &store, &nibbles(b"ca")).unwrap(), None);
assert!(validate(root.as_ref(), &store).is_ok());

// Every key at or after a start, ascending.
let from_cat: Vec<Path<Arity16>> = iter(root.as_ref(), &store, Some(&nibbles(b"cat")))
    .map(|item| item.unwrap().0)
    .collect();
assert_eq!(from_cat, vec![nibbles(b"cat"), nibbles(b"dog")]);

assert_eq!(remove(&mut root, &mut store, &nibbles(b"cat")).unwrap(), Some(b"meow".to_vec()));
remove_prefix(&mut root, &mut store, &nibbles(b"c")).unwrap();
assert_eq!(iter(root.as_ref(), &store, None).count(), 1);
```

Hashing takes a `TrieHasher`, which receives each node's leading path, partial
path, value, sibling count, and children's hashes, and returns the node's hash.
`hash` seals every inline edge with its node's hash on the way up and returns
the root's. The crate ships no hasher: the scheme is the adopter's.

## Nodes, edges, and stores

A trie is `Option<Node<V, E, A, S>>`; the empty trie is `None`. A node is a
partial path, an optional value, and a children map of edges `E`. There is no
leaf or branch distinction: a leaf is a node with no children. The
representation of the children map is chosen by a marker type `S`:

| Marker   | Container     | Trade                                                                                   |
| -------- | ------------- | --------------------------------------------------------------------------------------- |
| `Packed` | `PackedArray` | Pointer-sized, zero heap when empty; reallocates on an insert or remove that changes the child set, never on descent or hashing. |
| `Gapped` | `GappedArray` | Pointer-sized, with spare capacity so inserts and removes rarely reallocate.            |
| `Fixed`  | `FixedArray`  | Never reallocates and indexes in constant time, but every node carries a full-width array; at arity 16 a leaf is on the order of a kilobyte where a `Packed` leaf is its path and value. |

Proof nodes, built once and read, fit `Packed` outright. Which representation
suits stored nodes is a measurement; the `ops` bench under `benches/` is where
the numbers come from.

The `EdgeStore` trait is the adopter's side: how an edge resolves to a node
(`read`, returning a `StableDeref` handle that may borrow the edge or own an
`Arc`), and how a node moves between the *inline* state, where `as_inline`
returns it and `EdgeStore::hash` is `None`, and the *sealed* state, where the
edge carries the node's hash. The mutation algorithms `materialize` every edge on a mutated
path, which is the store's one chance to record that a persistent node is
being replaced; the hash walk seals inline edges. `materialize` is the only
fallible step of a mutation and every mutation performs its fallible calls
before its first structural change, so on `Err` the trie's contents are
unchanged and the caller may retry.

## Contracts worth knowing

- **Structural invariant.** For every trie reached only through `insert`,
  `remove`, and `remove_prefix`, a valueless node has at least two children
  and a childless node has a value. Constructors and `children_mut` do not
  check it, because adopters build other shapes on purpose; `validate`
  reports violations.
- **Hash validity.** A sealed edge's cached hash is the hash of its node at
  the full path where it was sealed, next to the siblings it had then, under
  the hasher that sealed it. A caller that moves a sealed subtree, or wants to
  hash under another hasher, calls `materialize_subtree` first.
- **Sibling-sensitive schemes.** A hasher may declare that a node's children
  hash differently depending on whether they have siblings (Ethereum's account
  storage root, where a lone storage child hashes as a standalone trie root).
  The walk rehashes a lone sealed child of such a parent, which is sufficient
  because a parent with two or more sealed children had that rule applied
  when each was sealed. The dependency supported is exactly "only child or
  not".
- **Value rewrite at hash time.** `TrieHasher::update_value` lets a scheme
  rewrite a node's stored value from its children's hashes before hashing
  (the same storage root, spliced into the account's stored bytes). A value
  written at such a level reads back as written until the next `hash`.
- **Memory.** Heap use is proportional to depth for every operation except
  `remove_prefix`, whose all-or-nothing contract holds the deleted subtree
  resident until all of it is materialized. That is bounded by the caller's
  own data; a caller that must cap the spike removes a large prefix in chunks
  through `iter` from a start and `remove`.

## Unsafe code

The crate uses `unsafe` where it removes a real memory or performance cost,
never to work around the borrow checker with an allocation. There are two
surfaces, each with its invariant stated in its module:

- `frames`: mutation and hashing descend by holding raw pointers to the nodes
  on the current path. The stack's API enforces strict last-in, first-out use,
  which is what Stacked Borrows requires of a walk that holds pointers into a
  parent's map while it works on a child.
- `chain`: lookup and iteration keep a stack of read handles in which each
  handle borrows the node its predecessor dereferences to, with the lifetimes
  erased so they share one allocation. Handles are released in reverse order,
  and the pointee stays put because every handle is `StableDeref`.

Both run under Miri, with strict provenance, over the model proptests against
three test stores: owning edges with reference handles, Firewood-shaped
`Box`/`Rc` edges with an enum handle, and edges that hold their node by value
inside the parent's map.

## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating, forwarded to `arity-arrays`. To compile a subset, disable defaults: `arity-tries = { version = "0.1", default-features = false, features = ["16"] }`. The `key` helpers need `16`. |
| `std` | | Forwards `std` to `arity-arrays` and `stable_deref_trait`. The crate is `no_std`-first. |

The arity features are **additive** and safe to combine. The test suite compiles
and runs only under the default (all-arity) feature set — run `cargo test`, not a
per-arity `cargo test --no-default-features --features 16`.

## MSRV

Minimum Supported Rust Version: **1.92**.

## Status

Pre-release. The `EdgeStore`, `ChildMap`, and `TrieHasher` traits and the shape
of `Node` are the surface that freezes on first adoption; while the crate is at
`0.x`, any change to them bumps the minor version.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
