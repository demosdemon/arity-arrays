#![no_main]

use arity_arrays::Arity128;
use arity_arrays::GappedArray;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run::<Arity128, GappedArray<Vec<u8>, Arity128>>(ops));
