# Faithful local Linux fuzzing: real glibc + ASAN + libFuzzer at native speed.
# Build/run for the HOST-NATIVE arch only — never pass --platform. Running an
# x86_64 ASAN binary under qemu emulation aborts in ASAN's allocator
# (sanitizer_allocator_primary32 CHECK). On Apple Silicon this runs arm64
# Linux natively (faithful for this arch-agnostic crate); on x86_64 hosts it
# runs CI's exact triple.
FROM rustlang/rust:nightly-bookworm

# A C toolchain/linker; rustc bundles the libFuzzer + ASAN runtimes.
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential \
    && rm -rf /var/lib/apt/lists/*

# Source-install cargo-fuzz so its default --target is the glibc triple (the
# musl binstall binary would default to a musl triple, where ASAN cannot link).
RUN cargo install cargo-fuzz --locked

WORKDIR /src/fuzz
# Keep build artifacts off the host-mounted tree (avoid darwin/linux object
# mixing); the repo is bind-mounted at /src at run time.
ENV CARGO_TARGET_DIR=/tmp/fuzz-target
