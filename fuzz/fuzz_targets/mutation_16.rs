#![no_main]

use arity_arrays::Arity16;
use arity_arrays::PackedArray;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run::<Arity16, PackedArray<Vec<u8>, Arity16>>(ops));
