#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../src/compact.rs"]
mod compact;

fuzz_target!(|input: compact::CompactFuzz| compact::compact_decode_run(input));
