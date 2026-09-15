#![no_std]

//! Path-compressed fixed-arity tries over `arity-arrays`.

extern crate alloc;

pub mod children;
pub mod key;
pub mod node;
pub mod path;

pub use arity_arrays::Arity;
pub use children::ChildMap;
pub use children::ChildStore;
pub use children::Fixed;
pub use children::Gapped;
pub use children::Packed;
pub use node::Node;
pub use node::drop_subtree;
pub use path::Path;
pub use path::PrefixOverlap;
pub use path::common_prefix;
pub use path::join;
