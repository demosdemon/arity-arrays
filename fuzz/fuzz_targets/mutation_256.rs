#![no_main]

use arity_arrays::Arity256;
use arity_arrays::PackedArray;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run::<Arity256, PackedArray<Vec<u8>, Arity256>>(ops));
