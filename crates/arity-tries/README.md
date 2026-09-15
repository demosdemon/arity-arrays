# arity-tries

Path-compressed fixed-arity tries over [`arity-arrays`](../arity-arrays).

This crate is `#![no_std]` but requires `alloc`.

## Status

Pre-release and under construction. What exists so far is the path type
(`Path`, `join`, `common_prefix`) and the arity-16 key helpers
(`key::nibbles`, `key::unnibble`).
