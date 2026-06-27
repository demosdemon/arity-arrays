//! `FixedArray<Option<T>, A>` → `PackedArray` → back is the identity, for every
//! arity, checked against a `BTreeMap` reference of the chosen slots.

use std::collections::BTreeMap;

use arity_arrays::Arity;
use arity_arrays::FixedArray;
use arity_arrays::PackedArray;
use arity_arrays::index::Niche;
use proptest::prelude::*;

fn check<A: Arity>(present: &BTreeMap<usize, u32>) {
    let mut src = FixedArray::<Option<u32>, A>::new();
    for (&i, &v) in present {
        let idx = A::Index::try_from_usize(i).expect("i < LEN");
        src[idx] = Some(v);
    }
    let packed = PackedArray::from(&src);
    // count + membership + values
    assert_eq!(packed.count(), present.len());
    for i in 0..A::LEN {
        let idx = A::Index::try_from_usize(i).expect("i < LEN");
        assert_eq!(packed.get(idx), present.get(&i));
    }
    // iter_present is ascending and matches the model (forward and reverse)
    let fwd: Vec<(usize, u32)> = packed
        .iter_present()
        .map(|(i, &v)| (i.as_usize(), v))
        .collect();
    let model: Vec<(usize, u32)> = present.iter().map(|(&i, &v)| (i, v)).collect();
    assert_eq!(fwd, model);
    let mut back: Vec<(usize, u32)> = packed
        .iter_present()
        .rev()
        .map(|(i, &v)| (i.as_usize(), v))
        .collect();
    back.reverse();
    assert_eq!(back, model);
    // owned round-trip is the identity
    let recovered: FixedArray<Option<u32>, A> = PackedArray::from(&src).into();
    for i in 0..A::LEN {
        let idx = A::Index::try_from_usize(i).expect("i < LEN");
        assert_eq!(*recovered.get(idx), present.get(&i).copied());
    }
}

macro_rules! roundtrip_for {
    ($test:ident, $arity:ty, $len:expr) => {
        proptest! {
            #[test]
            fn $test(entries in proptest::collection::vec((0usize..$len, any::<u32>()), 0..$len)) {
                let model: BTreeMap<usize, u32> = entries.into_iter().collect();
                check::<$arity>(&model);
            }
        }
    };
}

roundtrip_for!(arity8, arity_arrays::Arity8, 8);
roundtrip_for!(arity16, arity_arrays::Arity16, 16);
roundtrip_for!(arity32, arity_arrays::Arity32, 32);
roundtrip_for!(arity64, arity_arrays::Arity64, 64);
roundtrip_for!(arity128, arity_arrays::Arity128, 128);
roundtrip_for!(arity256, arity_arrays::Arity256, 256);
