#![no_main]

use arity_arrays::Arity16;
use arity_tries::Fixed;
use libfuzzer_sys::fuzz_target;

#[path = "../src/tries_common.rs"]
mod tries_common;
#[path = "../src/tries_shape.rs"]
mod tries_shape;

fuzz_target!(|input: (tries_shape::Shape, Vec<Vec<u8>>)| tries_shape::shape_run::<Arity16, Fixed>(input.0, input.1));
