#![no_main]

use arity_arrays::bitmap::U256;
use libfuzzer_sys::fuzz_target;

#[path = "../src/bitmap.rs"]
mod bitmap;

fuzz_target!(|bytes: [u8; 32]| bitmap::bitmap_roundtrip_run::<U256>(&bytes));
