#![no_main]

use arity_arrays::Arity8;
use arity_arrays::GappedArray;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run::<Arity8, GappedArray<Vec<u8>, Arity8>>(ops));
