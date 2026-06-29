//! Crate-internal `macro_rules!` that stamp the impls duplicated between the
//! sibling array representations (`PackedArray`, `GappedArray`). Each macro is
//! invoked once per representation, keeping the generated impls concrete and
//! auditable while removing verbatim copies. This mirrors the crate's existing
//! macro style (`impl_native_bitmap!` in arity-bitmap).

/// Emits the compile-time pointer-size witness for an array representation:
/// a feature-gated `SizeWitness` arity alias plus a `const _` assertion that
/// `$Ty<[u8; 32], SizeWitness>` is pointer-sized. Invoked once per module.
macro_rules! impl_size_witness {
    ($Ty:ident) => {
        // Compile-time guarantee: pointer-sized. Witnessed by whichever arity is
        // enabled (the property is generic over `A`; the marker is only a witness).
        #[cfg(feature = "8")]
        type SizeWitness = $crate::Arity8;
        #[cfg(all(not(feature = "8"), feature = "16"))]
        type SizeWitness = $crate::Arity16;
        #[cfg(all(not(feature = "8"), not(feature = "16"), feature = "32"))]
        type SizeWitness = $crate::Arity32;
        #[cfg(all(
            not(feature = "8"),
            not(feature = "16"),
            not(feature = "32"),
            feature = "64"
        ))]
        type SizeWitness = $crate::Arity64;
        #[cfg(all(
            not(feature = "8"),
            not(feature = "16"),
            not(feature = "32"),
            not(feature = "64"),
            feature = "128"
        ))]
        type SizeWitness = $crate::Arity128;
        #[cfg(all(
            not(feature = "8"),
            not(feature = "16"),
            not(feature = "32"),
            not(feature = "64"),
            not(feature = "128"),
            feature = "256"
        ))]
        type SizeWitness = $crate::Arity256;

        #[cfg(any(
            feature = "8",
            feature = "16",
            feature = "32",
            feature = "64",
            feature = "128",
            feature = "256"
        ))]
        const _: () = assert!(
            ::core::mem::size_of::<$Ty<[u8; 32], SizeWitness>>()
                == ::core::mem::size_of::<*const ()>()
        );
    };
}
