//! Crate-internal `macro_rules!` that stamp the impls duplicated between the
//! sibling array representations (`PackedArray`, `GappedArray`). Each macro is
//! invoked once per representation, keeping the generated impls concrete and
//! auditable while removing verbatim copies. This mirrors the crate's existing
//! macro style (`impl_native_bitmap!` in arity-bitmap).

/// Emits the compile-time pointer-size witness for an array representation:
/// a feature-gated `SizeWitness` arity alias plus a `const _` assertion that
/// `$Ty<[u8; 32], SizeWitness>` is pointer-sized. Invoked once per
/// representation.
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

/// Emits the gap-agnostic value impls (`PartialEq`/`Eq`/`Hash`/`Debug`) and the
/// thread-safety impls (`Send`/`Sync`/`UnwindSafe`/`RefUnwindSafe` for the
/// array, `Send`/`Sync` for its present-iterator `$Iter`). `$reason` is the
/// `#[expect]` justification for the iterator's raw pointer. Depends on `Arity`
/// being in scope and on the inherent `bitmap()`, `count()`, and
/// `iter_present()` methods of `$Ty`.
macro_rules! impl_dense_common {
    ($Ty:ident, $Iter:ident, $reason:literal) => {
        impl<T: PartialEq, A: Arity> PartialEq for $Ty<T, A> {
            fn eq(&self, other: &Self) -> bool {
                self.bitmap() == other.bitmap()
                    && self
                        .iter_present()
                        .map(|(_, v)| v)
                        .eq(other.iter_present().map(|(_, v)| v))
            }
        }

        impl<T: Eq, A: Arity> Eq for $Ty<T, A> {}

        impl<T: ::core::hash::Hash, A: Arity> ::core::hash::Hash for $Ty<T, A> {
            fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
                self.count().hash(state);
                for (i, v) in self.iter_present() {
                    i.as_usize().hash(state);
                    v.hash(state);
                }
            }
        }

        impl<T: ::core::fmt::Debug, A: Arity> ::core::fmt::Debug for $Ty<T, A> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_map()
                    .entries(self.iter_present().map(|(i, v)| (i.as_usize(), v)))
                    .finish()
            }
        }

        // SAFETY: the array exclusively owns its allocation; sending it across
        // threads is sound when `T: Send`.
        unsafe impl<T: Send, A: Arity> Send for $Ty<T, A> {}
        // SAFETY: a shared reference yields only `&T`; no interior mutability.
        unsafe impl<T: Sync, A: Arity> Sync for $Ty<T, A> {}

        // `NonNull` is `!UnwindSafe`; the array owns its data with no
        // shared/cyclic state, so these hold whenever `T` does.
        impl<T: ::core::panic::UnwindSafe, A: Arity> ::core::panic::UnwindSafe for $Ty<T, A> {}
        impl<T: ::core::panic::RefUnwindSafe, A: Arity> ::core::panic::RefUnwindSafe
            for $Ty<T, A>
        {
        }

        // The present-iterator holds a `*const T` (which suppresses the
        // auto-impls) but only ever yields `&T` (slice-like), so it is
        // `Send`/`Sync` exactly when `T: Sync`. The all-slots iterator borrows
        // `&$Ty`, so it derives `Send`/`Sync` automatically once the array is
        // `Sync`.
        #[expect(clippy::non_send_fields_in_send_ty, reason = $reason)]
        // SAFETY: the raw pointer is used only for shared reads for the lifetime
        // the iterator borrows its source array; it never aliases a mutable reference.
        unsafe impl<T: Sync, A: Arity> Send for $Iter<'_, T, A> {}
        // SAFETY: shared, read-only access; no interior mutability.
        unsafe impl<T: Sync, A: Arity> Sync for $Iter<'_, T, A> {}
    };
}

/// Emits the `serde_with` `Compact` wire-form adapter (`SerializeAs` +
/// `DeserializeAs`) for an array representation. `SerializeAs` streams
/// `iter_present()` via `PresentValues` without a temporary value `Vec`;
/// `DeserializeAs` reads a `(Vec<u8>, Vec<T>)` tuple, validates the bitmap
/// length and popcount, then reconstructs the array. Requires `Compact`,
/// `PresentValues`, `Bitmap`, `Arity`, `FixedArray`, the `serde`/`serde_with`
/// traits, and `COMPACT_LEN_ERR`/`COMPACT_POPCOUNT_ERR` in scope.
#[cfg(feature = "serde_with")]
macro_rules! impl_compact_adapter {
    ($Ty:ident) => {
        impl<T: Serialize, A: Arity> SerializeAs<$Ty<T, A>> for Compact {
            fn serialize_as<S: ::serde::Serializer>(
                source: &$Ty<T, A>,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                let mut buf = alloc::vec![0u8; <A::Bitmap as Bitmap>::BYTES];
                source.bitmap().to_le_bytes(&mut buf);
                (buf, PresentValues(|| source.iter_present().map(|(_, v)| v)))
                    .serialize(serializer)
            }
        }

        impl<'de, T: Deserialize<'de>, A: Arity> DeserializeAs<'de, $Ty<T, A>> for Compact {
            fn deserialize_as<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<$Ty<T, A>, D::Error> {
                let (buf, values): (Vec<u8>, Vec<T>) = Deserialize::deserialize(deserializer)?;
                if buf.len() != <A::Bitmap as Bitmap>::BYTES {
                    return Err(::serde::de::Error::invalid_length(buf.len(), &COMPACT_LEN_ERR));
                }
                let bitmap = <A::Bitmap as Bitmap>::from_le_bytes(&buf);
                if bitmap.count_ones() as usize != values.len() {
                    return Err(::serde::de::Error::custom(COMPACT_POPCOUNT_ERR));
                }
                let mut out = FixedArray::<Option<T>, A>::new();
                for (index, value) in bitmap.bits().zip(values) {
                    out[index] = Some(value);
                }
                Ok($Ty::from(out))
            }
        }
    };
}

/// Emits the logical-form serde impls (a sequence of strictly ascending
/// `(index, value)` pairs) for an array representation. `$label` prefixes the
/// strictly-ascending
/// error message. Gated on `feature = "serde"`. Requires `FixedArray` and
/// `Arity` in scope at the invocation site.
macro_rules! impl_logical_serde {
    ($Ty:ident, $label:literal) => {
        #[cfg(feature = "serde")]
        impl<T: ::serde::Serialize, A: Arity> ::serde::Serialize for $Ty<T, A>
        where
            A::Index: ::serde::Serialize,
        {
            fn serialize<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                // Logical form: a sequence of `(index, value)` pairs, ascending.
                serializer.collect_seq(self.iter_present())
            }
        }

        #[cfg(feature = "serde")]
        impl<'de, T: ::serde::Deserialize<'de>, A: Arity> ::serde::Deserialize<'de> for $Ty<T, A>
        where
            A::Index: ::serde::Deserialize<'de>,
        {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<Self, D::Error> {
                struct PairsVisitor<T, A>(::core::marker::PhantomData<(T, A)>);

                impl<'de, T: ::serde::Deserialize<'de>, A: Arity> ::serde::de::Visitor<'de>
                    for PairsVisitor<T, A>
                where
                    A::Index: ::serde::Deserialize<'de>,
                {
                    type Value = $Ty<T, A>;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        f.write_str(
                            "a sequence of (index, value) pairs with strictly ascending indices",
                        )
                    }

                    fn visit_seq<S: ::serde::de::SeqAccess<'de>>(
                        self,
                        mut seq: S,
                    ) -> ::core::result::Result<Self::Value, S::Error> {
                        let mut out = FixedArray::<Option<T>, A>::new();
                        let mut last: Option<usize> = None;
                        while let Some((index, value)) = seq.next_element::<(A::Index, T)>()? {
                            let i = index.as_usize();
                            if last.is_some_and(|prev| i <= prev) {
                                return Err(::serde::de::Error::custom(concat!(
                                    $label,
                                    " indices must be strictly ascending"
                                )));
                            }
                            last = Some(i);
                            out[index] = Some(value);
                        }
                        Ok($Ty::from(out))
                    }
                }

                deserializer.deserialize_seq(PairsVisitor(::core::marker::PhantomData))
            }
        }
    };
}
