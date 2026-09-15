#![no_std]

//! Path-compressed fixed-arity tries over `arity-arrays`.
//!
//! One node type, [`Node`], and the algorithms over it, each generic over an
//! application-provided [`EdgeStore`] and, for hashing, a [`TrieHasher`]:
//! [`get`], [`insert`], [`remove`], [`remove_prefix`], [`iter()`], [`visit`],
//! [`validate()`], and [`hash()`]. No walk recurses on the call stack. See the
//! crate README for the contracts an adopter relies on.

extern crate alloc;

mod chain;
pub mod children;
mod frames;
pub mod hash;
pub mod iter;
pub mod key;
pub mod memory;
pub mod node;
pub mod ops;
pub mod path;
pub mod store;
#[cfg(test)]
mod testing;
pub mod validate;

pub use arity_arrays::Arity;
#[cfg(feature = "8")]
pub use arity_arrays::Arity8;
#[cfg(feature = "16")]
pub use arity_arrays::Arity16;
#[cfg(feature = "32")]
pub use arity_arrays::Arity32;
#[cfg(feature = "64")]
pub use arity_arrays::Arity64;
#[cfg(feature = "128")]
pub use arity_arrays::Arity128;
#[cfg(feature = "256")]
pub use arity_arrays::Arity256;
pub use children::ChildMap;
pub use children::ChildStore;
pub use children::Fixed;
pub use children::Gapped;
pub use children::Packed;
pub use hash::HashInput;
pub use hash::TrieHasher;
pub use hash::hash;
pub use hash::hash_sealed_node;
pub use iter::Iter;
pub use iter::iter;
pub use iter::visit;
pub use memory::InMemory;
pub use memory::MemEdge;
pub use node::Node;
pub use node::drop_subtree;
pub use ops::get;
pub use ops::get_node;
pub use ops::insert;
pub use ops::materialize_subtree;
pub use ops::remove;
pub use ops::remove_prefix;
pub use path::Path;
pub use path::PrefixOverlap;
pub use path::common_prefix;
pub use path::join;
pub use store::EdgeStore;
pub use store::NodeRef;
pub use validate::ValidateError;
pub use validate::Violation;
pub use validate::ViolationKind;
pub use validate::validate;

/// The crate README's usage example, compiled as a doctest so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
