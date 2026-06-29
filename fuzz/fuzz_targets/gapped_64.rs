#![no_main]

use arity_arrays::Arity64;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run_gapped::<Arity64>(ops));
