#![no_main]

use arity_arrays::Arity256;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run_gapped::<Arity256>(ops));
