#![no_std]

//! Path-compressed fixed-arity tries over `arity-arrays`.

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
