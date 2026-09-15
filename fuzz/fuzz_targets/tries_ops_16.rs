#![no_main]

use arity_arrays::Arity16;
use arity_tries::Fixed;
use libfuzzer_sys::fuzz_target;

#[path = "../src/tries_common.rs"]
mod tries_common;
#[path = "../src/tries_ops.rs"]
mod tries_ops;

fuzz_target!(|ops: Vec<tries_ops::Op>| tries_ops::ops_run::<Arity16, Fixed>(ops));
