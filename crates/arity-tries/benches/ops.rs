//! `insert`, `remove`, `remove_prefix`, and `hash` over the in-memory store,
//! for `Packed`, `Gapped`, and `Fixed` at arity 16 and `Packed` at arity 256,
//! on chain (deep), bushy (broad), and tapered shapes. It measures descent
//! and the hash walk per node, and it is what confirms that no representation
//! pays a per-level reallocation on write.

#[path = "../../arity-arrays/benches/quick_criterion.rs"]
mod quick;

use std::hint::black_box;

use arity_arrays::Arity;
use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::index::Niche;
use arity_tries::ChildStore;
use arity_tries::Fixed;
use arity_tries::Gapped;
use arity_tries::HashInput;
use arity_tries::InMemory;
use arity_tries::MemEdge;
use arity_tries::Node;
use arity_tries::Packed;
use arity_tries::TrieHasher;
use arity_tries::hash;
use arity_tries::insert;
use arity_tries::remove;
use arity_tries::remove_prefix;
use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::criterion_group;
use quick::quick_criterion;

type Store = InMemory<u64>;
type N<A, S> = Node<u32, MemEdge<u32, A, S, u64>, A, S>;

#[derive(Clone, Copy, Debug)]
enum Shape {
    /// Keys sharing a long prefix: one deep path.
    Chain,
    /// Short keys spread over every index: wide and shallow.
    Bushy,
    /// Wide at the top, narrowing with depth.
    Tapered,
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Chain => "chain",
            Self::Bushy => "bushy",
            Self::Tapered => "tapered",
        })
    }
}

const SHAPES: &[Shape] = &[Shape::Chain, Shape::Bushy, Shape::Tapered];
const KEYS: usize = 512;

fn idx<A: Arity>(b: usize) -> A::Index {
    A::Index::try_from_usize(b % A::LEN).expect("in range")
}

/// A deterministic key set for the shape, `KEYS` keys long.
fn keys<A: Arity>(shape: Shape) -> Vec<Vec<A::Index>> {
    let mut seed = 0x9e37_79b9u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };
    (0..KEYS)
        .map(|i| match shape {
            Shape::Chain => (0..i % 64)
                .map(|_| idx::<A>(0))
                .chain([idx::<A>(1 + i % (A::LEN - 1))])
                .collect(),
            Shape::Bushy => (0..3).map(|_| idx::<A>(next())).collect(),
            Shape::Tapered => {
                let depth = 2 + i % 6;
                (0..depth)
                    .map(|d| idx::<A>(next() % (A::LEN >> d.min(3)).max(1)))
                    .collect()
            }
        })
        .collect()
}

fn build<A: Arity, S: ChildStore<A>>(keys: &[Vec<A::Index>]) -> (Option<N<A, S>>, Store) {
    let (mut root, mut store) = (None, Store::default());
    for (i, k) in keys.iter().enumerate() {
        insert(&mut root, &mut store, k, u32::try_from(i).expect("small")).expect("infallible");
    }
    (root, store)
}

/// A cheap position-dependent hasher, so the walk's cost dominates.
struct Fnv;

impl<A: Arity> TrieHasher<u32, A> for Fnv {
    type Hash = u64;

    fn hash_node<'a, C>(&self, input: HashInput<'a, u32, A, C>) -> u64
    where
        C: Iterator<Item = (A::Index, &'a u64)> + Clone,
    {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut feed = |b: u8| {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        input
            .leading_path
            .iter()
            .chain(input.partial_path)
            .for_each(|i| feed(u8::try_from(i.as_usize()).expect("index fits a byte")));
        input
            .value
            .map(|v| v.to_le_bytes())
            .unwrap_or_default()
            .iter()
            .for_each(|b| feed(*b));
        for (i, child) in input.children {
            feed(u8::try_from(i.as_usize()).expect("index fits a byte"));
            child.to_le_bytes().iter().for_each(|b| feed(*b));
        }
        h
    }
}

fn bench_store<A: Arity, S: ChildStore<A>>(c: &mut Criterion, arity: &str, rep: &str) {
    for &shape in SHAPES {
        let keys = keys::<A>(shape);
        let id = || BenchmarkId::new(rep, shape);
        c.benchmark_group(format!("tries/{arity}/insert"))
            .bench_with_input(id(), &keys, |b, keys| {
                b.iter_batched(
                    || (),
                    |()| black_box(build::<A, S>(keys)),
                    BatchSize::PerIteration,
                );
            });
        c.benchmark_group(format!("tries/{arity}/remove"))
            .bench_with_input(id(), &keys, |b, keys| {
                b.iter_batched(
                    || build::<A, S>(keys),
                    |(mut root, mut store)| {
                        for k in keys {
                            black_box(remove(&mut root, &mut store, k).expect("infallible"));
                        }
                        root
                    },
                    BatchSize::PerIteration,
                );
            });
        c.benchmark_group(format!("tries/{arity}/remove_prefix"))
            .bench_with_input(id(), &keys, |b, keys| {
                b.iter_batched(
                    || build::<A, S>(keys),
                    |(mut root, mut store)| {
                        remove_prefix(&mut root, &mut store, &[]).expect("infallible");
                        root
                    },
                    BatchSize::PerIteration,
                );
            });
        c.benchmark_group(format!("tries/{arity}/hash"))
            .bench_with_input(id(), &keys, |b, keys| {
                b.iter_batched(
                    || build::<A, S>(keys),
                    |(mut root, mut store)| {
                        let h = hash(root.as_mut().expect("root"), &mut store, &Fnv)
                            .expect("infallible");
                        (black_box(h), root)
                    },
                    BatchSize::PerIteration,
                );
            });
    }
}

fn arity16(c: &mut Criterion) {
    bench_store::<Arity16, Packed>(c, "arity16", "packed");
    bench_store::<Arity16, Gapped>(c, "arity16", "gapped");
    bench_store::<Arity16, Fixed>(c, "arity16", "fixed");
}

fn arity256(c: &mut Criterion) {
    bench_store::<Arity256, Packed>(c, "arity256", "packed");
}

criterion_group!(
    name = benches;
    config = quick_criterion();
    targets = arity16, arity256
);

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
