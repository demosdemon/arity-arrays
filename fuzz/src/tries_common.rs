//! Shared pieces of the `tries_ops_*` and `tries_shape_*` fuzz harnesses:
//! the store and node types, index mapping, the test hasher, and a contents
//! dump.
//!
//! `#[path]`-included by each `fuzz_targets/tries_*.rs` beside the harness it
//! drives; the fuzz crate has no `[lib]`, so these are per-binary module
//! includes.

use arity_arrays::index::Niche;
use arity_tries::Arity;
use arity_tries::ChildStore;
use arity_tries::HashInput;
use arity_tries::InMemory;
use arity_tries::MemEdge;
use arity_tries::Node;
use arity_tries::Path;
use arity_tries::TrieHasher;
use arity_tries::iter;

pub type Store = InMemory<u64>;
pub type N<A, S> = Node<u32, MemEdge<u32, A, S, u64>, A, S>;

/// Map an arbitrary byte to a valid index for arity `A`. The mask is total
/// for a power-of-two width, so `try_from_usize` always returns `Some`.
pub fn idx<A: Arity>(b: u8) -> A::Index {
    <A::Index as Niche>::try_from_usize((b as usize) & (A::LEN - 1)).unwrap()
}

pub fn path<A: Arity>(bytes: &[u8]) -> Path<A> {
    bytes.iter().map(|&b| idx::<A>(b)).collect()
}

/// A 64-bit FNV-1a hasher over the whole input, sibling-sensitive at depth
/// one and rewriting values at depth two, so both walk special cases run.
pub struct Fnv;

impl Fnv {
    fn feed(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= u64::from(*b);
            *h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn digest<'a, I: Niche>(children: impl Iterator<Item = (I, &'a u64)>) -> u64 {
        let mut h = 0x9e37_79b9_7f4a_7c15;
        for (i, child) in children {
            Self::feed(&mut h, &[i.as_usize() as u8]);
            Self::feed(&mut h, &child.to_le_bytes());
        }
        h
    }
}

impl<A: Arity> TrieHasher<u32, A> for Fnv {
    type Hash = u64;

    fn hash_node<'a, C>(&self, input: HashInput<'a, u32, A, C>) -> u64
    where
        C: Iterator<Item = (A::Index, &'a u64)> + Clone,
    {
        let mut h = 0xcbf2_9ce4_8422_2325;
        Self::feed(&mut h, A::Index::as_u8_slice(input.leading_path));
        Self::feed(&mut h, &[0xff]);
        Self::feed(&mut h, A::Index::as_u8_slice(input.partial_path));
        Self::feed(&mut h, &[0xff]);
        match input.value {
            Some(v) => Self::feed(&mut h, &v.to_le_bytes()),
            None => Self::feed(&mut h, &[0xfe]),
        }
        let lone = input.leading_path.len() == 2 && input.siblings == 1;
        Self::feed(&mut h, &[u8::from(lone)]);
        Self::feed(&mut h, &Self::digest(input.children).to_le_bytes());
        h
    }

    fn sibling_sensitive(&self, leading_path: &[A::Index], partial_path: &[A::Index]) -> bool {
        leading_path.len() + partial_path.len() == 1
    }

    fn update_value<'a, C>(&self, leading_path: &[A::Index], partial_path: &[A::Index], children: C, value: &mut u32)
    where
        C: Iterator<Item = (A::Index, &'a u64)> + Clone,
    {
        if leading_path.len() + partial_path.len() == 2 {
            *value = (Self::digest(children) & u64::from(u32::MAX)) as u32;
        }
    }
}

pub fn contents<A: Arity, S: ChildStore<A>>(root: Option<&N<A, S>>, store: &Store) -> Vec<(Vec<u8>, u32)> {
    iter(root, store, None)
        .map(|item| {
            let (p, v) = item.unwrap();
            (p.as_bytes().to_vec(), v)
        })
        .collect()
}
