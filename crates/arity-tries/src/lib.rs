#![no_std]

//! Path-compressed fixed-arity tries over `arity-arrays`.

extern crate alloc;

mod chain;
pub mod children;
pub mod key;
pub mod memory;
pub mod node;
pub mod ops;
pub mod path;
pub mod store;

pub use arity_arrays::Arity;
pub use children::ChildMap;
pub use children::ChildStore;
pub use children::Fixed;
pub use children::Gapped;
pub use children::Packed;
pub use memory::InMemory;
pub use memory::MemEdge;
pub use node::Node;
pub use node::drop_subtree;
pub use ops::get;
pub use ops::get_node;
pub use path::Path;
pub use path::PrefixOverlap;
pub use path::common_prefix;
pub use path::join;
pub use store::EdgeStore;
pub use store::NodeRef;
