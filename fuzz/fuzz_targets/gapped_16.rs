#![no_main]

use arity_arrays::Arity16;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run_gapped::<Arity16>(ops));
