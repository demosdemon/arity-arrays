//! Codegen probe backing the LTO finding in the crate README: does the hot
//! `PackedArray::get` path emit any cross-crate call that `#[inline]` does not
//! already eliminate? Re-run with:
//!
//! ```text
//! cargo asm --profile lto-probe -p arity-arrays --all-features \
//!     --example inline_probe probe_packed_get
//! ```
//!
//! Compare against the same command with `--release`. Both bodies should be
//! call-free.
use arity_arrays::Arity16;
use arity_arrays::PackedArray;
use arity_arrays::index::U4;

/// Probe: the hot `PackedArray::get` path, which crosses from `arity-arrays`
/// into `arity-bitmap` (`test`/`rank`) and `arity-index` (`Niche::as_usize`).
///
/// `extern "Rust"` (not `"C"`) is deliberate: `U4` is a niche type, not
/// FFI-safe, so this stays the default Rust ABI made explicit — `#[no_mangle]`
/// alone leaves the ABI implicit, which `clippy::no_mangle_with_rust_abi`
/// flags.
#[unsafe(no_mangle)]
pub extern "Rust" fn probe_packed_get(p: &PackedArray<u32, Arity16>, i: U4) -> Option<&u32> {
    p.get(i)
}

fn main() {}
