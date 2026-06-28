//! Shared harness for the `compact_decode` fuzz target: drive the untrusted
//! `Compact` deserialize path (arbitrary bytes -> allocation) for `Arity256`.
//!
//! `#[path]`-included by `fuzz_targets/compact_decode.rs`.

use arbitrary::Arbitrary;
use arity_arrays::Arity256;
use arity_arrays::Compact;
use arity_arrays::PackedArray;
use serde_with::de::DeserializeAsWrap;
use serde_with::ser::SerializeAsWrap;

// Arity256 bitmap width is 256 bits == 32 bytes.
const BYTES: usize = 32;

type Pa = PackedArray<Vec<u8>, Arity256>;
type Wrap = DeserializeAsWrap<Pa, Compact>;

#[derive(Arbitrary, Debug)]
pub enum CompactFuzz {
    /// Controlled valid + invalid tuples — reaches Compact's length/popcount
    /// checks reliably.
    Structured(DecodeInput),
    /// Truly arbitrary bytes — exercises the postcard format layer plus Compact
    /// validation on garbage.
    Raw(Vec<u8>),
}

#[derive(Arbitrary, Debug)]
pub struct DecodeInput {
    pub bitmap: [u8; BYTES],
    pub len_perturb: i8,
    pub values: Vec<Vec<u8>>,
    pub align_to_popcount: bool,
}

pub fn compact_decode_run(input: CompactFuzz) {
    let bytes = match input {
        CompactFuzz::Raw(bytes) => bytes,
        CompactFuzz::Structured(mut d) => {
            // Bitmap byte buffer, length perturbed away from BYTES sometimes
            // to hit Compact's length-reject branch.
            let mut bm = d.bitmap.to_vec();
            let target = (BYTES as i32 + i32::from(d.len_perturb)).clamp(0, 64) as usize;
            bm.resize(target, 0);
            // When aligned and the buffer is the right width, make the value
            // count equal the popcount so the accept path is reachable.
            // Otherwise the popcount-reject branch is the likely outcome.
            if d.align_to_popcount && bm.len() == BYTES {
                let popcount = bm.iter().map(|b| b.count_ones()).sum::<u32>() as usize;
                d.values.resize(popcount, Vec::new());
            }
            postcard::to_allocvec(&(bm, d.values)).unwrap()
        }
    };

    // Decode is allowed to fail: graceful rejection (Err) is the expected
    // outcome for malformed input. The property is no panic / no UB.
    let Ok(p1) = postcard::from_bytes::<Wrap>(&bytes).map(Wrap::into_inner) else {
        return;
    };

    // Accepted input: a canonical re-encode must round-trip to an equal value.
    let canon = postcard::to_allocvec(&SerializeAsWrap::<Pa, Compact>::new(&p1)).unwrap();
    let p2 = postcard::from_bytes::<Wrap>(&canon).unwrap().into_inner();
    assert_eq!(p1, p2);
}
