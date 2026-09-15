//! Model-based properties over every arity, representation, and store.

mod common;

use arity_arrays::Arity8;
use arity_arrays::Arity16;
use arity_arrays::Arity32;
use arity_arrays::Arity64;
use arity_arrays::Arity128;
use arity_arrays::Arity256;
use arity_tries::Fixed;
use arity_tries::Gapped;
use arity_tries::InMemory;
use arity_tries::Packed;
use arity_tries::hash;
use arity_tries::materialize_subtree;
use common::*;
use proptest::prelude::*;

macro_rules! model_tests {
    ($($name:ident: $arity:ty, $rep:ty, $store:ty;)*) => {$(
        proptest! {
            #[test]
            fn $name(ops in ops(key::<$arity>(4), 32), start in key::<$arity>(5)) {
                run_model::<$arity, $rep, $store>(&ops, &start)?;
            }
        }
    )*};
}

model_tests! {
    in_memory_packed_arity8: Arity8, Packed, InMemory<H>;
    in_memory_gapped_arity8: Arity8, Gapped, InMemory<H>;
    in_memory_fixed_arity8: Arity8, Fixed, InMemory<H>;
    in_memory_packed_arity16: Arity16, Packed, InMemory<H>;
    in_memory_gapped_arity16: Arity16, Gapped, InMemory<H>;
    in_memory_fixed_arity16: Arity16, Fixed, InMemory<H>;
    in_memory_packed_arity32: Arity32, Packed, InMemory<H>;
    in_memory_gapped_arity32: Arity32, Gapped, InMemory<H>;
    in_memory_fixed_arity32: Arity32, Fixed, InMemory<H>;
    in_memory_packed_arity64: Arity64, Packed, InMemory<H>;
    in_memory_gapped_arity64: Arity64, Gapped, InMemory<H>;
    in_memory_fixed_arity64: Arity64, Fixed, InMemory<H>;
    in_memory_packed_arity128: Arity128, Packed, InMemory<H>;
    in_memory_gapped_arity128: Arity128, Gapped, InMemory<H>;
    in_memory_fixed_arity128: Arity128, Fixed, InMemory<H>;
    in_memory_packed_arity256: Arity256, Packed, InMemory<H>;
    in_memory_gapped_arity256: Arity256, Gapped, InMemory<H>;
    in_memory_fixed_arity256: Arity256, Fixed, InMemory<H>;

    rc_packed_arity8: Arity8, Packed, RcStore;
    rc_gapped_arity8: Arity8, Gapped, RcStore;
    rc_fixed_arity8: Arity8, Fixed, RcStore;
    rc_packed_arity16: Arity16, Packed, RcStore;
    rc_gapped_arity16: Arity16, Gapped, RcStore;
    rc_fixed_arity16: Arity16, Fixed, RcStore;
    rc_packed_arity32: Arity32, Packed, RcStore;
    rc_gapped_arity32: Arity32, Gapped, RcStore;
    rc_fixed_arity32: Arity32, Fixed, RcStore;
    rc_packed_arity64: Arity64, Packed, RcStore;
    rc_gapped_arity64: Arity64, Gapped, RcStore;
    rc_fixed_arity64: Arity64, Fixed, RcStore;
    rc_packed_arity128: Arity128, Packed, RcStore;
    rc_gapped_arity128: Arity128, Gapped, RcStore;
    rc_fixed_arity128: Arity128, Fixed, RcStore;
    rc_packed_arity256: Arity256, Packed, RcStore;
    rc_gapped_arity256: Arity256, Gapped, RcStore;
    rc_fixed_arity256: Arity256, Fixed, RcStore;

    value_packed_arity8: Arity8, Packed, ValueStore;
    value_gapped_arity8: Arity8, Gapped, ValueStore;
    value_packed_arity16: Arity16, Packed, ValueStore;
    value_gapped_arity16: Arity16, Gapped, ValueStore;
    value_packed_arity32: Arity32, Packed, ValueStore;
    value_gapped_arity32: Arity32, Gapped, ValueStore;
    value_packed_arity64: Arity64, Packed, ValueStore;
    value_gapped_arity64: Arity64, Gapped, ValueStore;
    value_packed_arity128: Arity128, Packed, ValueStore;
    value_gapped_arity128: Arity128, Gapped, ValueStore;
    value_packed_arity256: Arity256, Packed, ValueStore;
    value_gapped_arity256: Arity256, Gapped, ValueStore;
}

macro_rules! hash_tests {
    ($($name:ident: $arity:ty, $rep:ty, $store:ty;)*) => {$(
        proptest! {
            #[test]
            fn $name(ops in ops(narrow_key::<$arity>(), 40)) {
                run_hash::<$arity, $rep, $store>(&ops, &Fnv::SENSITIVE)?;
                run_hash::<$arity, $rep, $store>(&ops, &Fnv::REWRITING)?;
            }
        }
    )*};
}

hash_tests! {
    hash_in_memory_packed_arity16: Arity16, Packed, InMemory<H>;
    hash_in_memory_fixed_arity16: Arity16, Fixed, InMemory<H>;
    hash_rc_packed_arity16: Arity16, Packed, RcStore;
    hash_rc_fixed_arity16: Arity16, Fixed, RcStore;
    hash_value_packed_arity16: Arity16, Packed, ValueStore;
    hash_in_memory_packed_arity256: Arity256, Packed, InMemory<H>;
    hash_rc_packed_arity256: Arity256, Packed, RcStore;
    hash_value_gapped_arity256: Arity256, Gapped, ValueStore;
}

macro_rules! atomic_tests {
    ($($name:ident: $arity:ty, $rep:ty, $store:ty;)*) => {$(
        proptest! {
            #[test]
            fn $name(
                ops in ops(key::<$arity>(4), 30),
                op in ops(key::<$arity>(4), 2).prop_filter("one op", |v| v.len() == 1),
                fail_at in 1usize..6,
                fail_read in any::<bool>(),
            ) {
                run_atomic::<$arity, $rep, $store>(&ops, &op[0], fail_at, fail_read)?;
            }
        }
    )*};
}

atomic_tests! {
    atomic_in_memory_packed_arity16: Arity16, Packed, InMemory<H>;
    atomic_in_memory_fixed_arity16: Arity16, Fixed, InMemory<H>;
    atomic_rc_packed_arity16: Arity16, Packed, RcStore;
    atomic_rc_fixed_arity16: Arity16, Fixed, RcStore;
    atomic_value_gapped_arity16: Arity16, Gapped, ValueStore;
    atomic_rc_packed_arity256: Arity256, Packed, RcStore;
    atomic_value_packed_arity256: Arity256, Packed, ValueStore;
}

proptest! {
    /// Two hashers sharing the output type: switching after
    /// `materialize_subtree` matches a fresh build under the second, and
    /// switching without it does not.
    #[test]
    fn one_hasher_per_trie_arity16(ops in ops(key::<Arity16>(4), 30)) {
        let (mut root, mut store, mut model) = (None, RcStore::default(), Model::new());
        for op in &ops {
            apply::<Arity16, Packed, RcStore>(&mut root, &mut store, &mut model, &Fnv::PLAIN, op)?;
        }
        let Some(node) = root.as_mut() else { return Ok(()) };
        hash(node, &mut store, &Fnv::PLAIN).expect("store ok");
        let expected = fresh_hash::<Arity16, Packed, RcStore>(&model, &Fnv::SEEDED);
        if !node.is_leaf() {
            prop_assert_ne!(Some(hash(node, &mut store, &Fnv::SEEDED).expect("store ok")), expected);
        }
        materialize_subtree(node, &mut store).expect("store ok");
        prop_assert_eq!(Some(hash(node, &mut store, &Fnv::SEEDED).expect("store ok")), expected);
    }
}
