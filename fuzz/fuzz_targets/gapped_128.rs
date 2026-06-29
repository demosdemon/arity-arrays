#![no_main]

use arity_arrays::Arity128;
use libfuzzer_sys::fuzz_target;

#[path = "../src/mutation.rs"]
mod mutation;

fuzz_target!(|ops: Vec<mutation::Op>| mutation::mutation_run_gapped::<Arity128>(ops));
