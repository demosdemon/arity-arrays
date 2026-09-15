#![no_main]

use arity_arrays::Arity256;
use arity_tries::Packed;
use libfuzzer_sys::fuzz_target;

#[path = "../src/tries_common.rs"]
mod tries_common;
#[path = "../src/tries_shape.rs"]
mod tries_shape;

fuzz_target!(|input: (tries_shape::Shape, Vec<Vec<u8>>)| tries_shape::shape_run::<Arity256, Packed>(input.0, input.1));
