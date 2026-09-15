#![no_std]

//! Path-compressed fixed-arity tries over `arity-arrays`.

extern crate alloc;

pub mod key;
pub mod path;

pub use arity_arrays::Arity;
pub use path::Path;
pub use path::PrefixOverlap;
pub use path::common_prefix;
pub use path::join;
