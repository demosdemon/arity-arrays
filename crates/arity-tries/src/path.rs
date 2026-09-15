//! Owned index paths and the prefix arithmetic the algorithms share.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;
use core::hash::Hash;
use core::hash::Hasher;
use core::ops::Deref;

use arity_arrays::Arity;
use arity_arrays::index::Niche;

/// An owned, immutable run of indices: a node's partial path, or a full key.
///
/// The representation is private so that a small-buffer form stored in place
/// can replace the boxed slice without an API break. The algorithms take
/// `&[A::Index]` for keys and never require an owned path from the caller.
pub struct Path<A: Arity>(Box<[A::Index]>);

impl<A: Arity> Path<A> {
    /// The empty path. Does not allocate.
    #[must_use]
    pub fn new() -> Self {
        Self(Box::default())
    }

    /// One index per byte, validated with [`Niche::try_from_slice`]; `None` if
    /// any byte is `>= A::LEN`.
    #[must_use]
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        A::Index::try_from_slice(bytes).map(Self::from)
    }

    /// The indices as a slice; the `Deref` target, usable in `const`
    /// contexts.
    #[must_use]
    pub const fn as_slice(&self) -> &[A::Index] {
        &self.0
    }

    /// The path as bytes, one per index. A free reinterpretation.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        A::Index::as_u8_slice(&self.0)
    }
}

impl<A: Arity> Default for Path<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Arity> Clone for Path<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<A: Arity> Deref for Path<A> {
    type Target = [A::Index];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A: Arity> AsRef<[A::Index]> for Path<A> {
    fn as_ref(&self) -> &[A::Index] {
        &self.0
    }
}

impl<A: Arity> From<&[A::Index]> for Path<A> {
    fn from(slice: &[A::Index]) -> Self {
        Self(slice.into())
    }
}

impl<A: Arity> From<Vec<A::Index>> for Path<A> {
    fn from(vec: Vec<A::Index>) -> Self {
        Self(vec.into_boxed_slice())
    }
}

impl<A: Arity> FromIterator<A::Index> for Path<A> {
    fn from_iter<I: IntoIterator<Item = A::Index>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<A: Arity> PartialEq for Path<A> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<A: Arity> Eq for Path<A> {}

impl<A: Arity> PartialOrd for Path<A> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<A: Arity> Ord for Path<A> {
    /// Lexicographic, so a path sorts before every path it is a prefix of.
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<A: Arity> Hash for Path<A>
where
    A::Index: Hash,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<A: Arity> fmt::Debug for Path<A> {
    /// Prints the indices as decimal integers.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|i| i.as_usize()))
            .finish()
    }
}

/// The concatenation `head ++ [index] ++ tail`.
///
/// A free function over slices because both inputs are borrowed from nodes.
/// It is the collapse step of a removal: a parent's partial path, the index
/// of its only child, and that child's partial path become the merged node's
/// partial path.
#[must_use]
pub fn join<A: Arity>(head: &[A::Index], index: A::Index, tail: &[A::Index]) -> Path<A> {
    let mut vec = Vec::with_capacity(head.len() + 1 + tail.len());
    vec.extend_from_slice(head);
    vec.push(index);
    vec.extend_from_slice(tail);
    Path::from(vec)
}

/// The three-way split of two slices around their longest common prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixOverlap<'a, I> {
    /// The longest common prefix of both inputs.
    pub shared: &'a [I],
    /// What remains of the first input after `shared`.
    pub unique_a: &'a [I],
    /// What remains of the second input after `shared`.
    pub unique_b: &'a [I],
}

/// Splits `a` and `b` around their longest common prefix.
///
/// The first step of a descent: `a` is the unconsumed key and `b` is the
/// current node's partial path, and which of `unique_a` and `unique_b` are
/// empty decides the case.
#[must_use]
pub fn common_prefix<'a, I: PartialEq>(a: &'a [I], b: &'a [I]) -> PrefixOverlap<'a, I> {
    let shared_len = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    PrefixOverlap {
        shared: &a[..shared_len],
        unique_a: &a[shared_len..],
        unique_b: &b[shared_len..],
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use arity_arrays::Arity16;
    use arity_arrays::Arity256;
    use arity_arrays::index::U4;

    use super::*;

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    #[test]
    fn new_is_empty_and_default() {
        let p = Path::<Arity16>::new();
        assert_eq!(p.len(), 0);
        assert_eq!(p, Path::default());
        assert_eq!(&*p, &[] as &[U4]);
    }

    #[test]
    fn try_from_bytes_validates_every_byte() {
        let ok = Path::<Arity16>::try_from_bytes(&[0, 15, 3]).expect("in range");
        assert_eq!(&*ok, &[u4(0), u4(15), u4(3)]);
        assert!(Path::<Arity16>::try_from_bytes(&[0, 16]).is_none());
        assert!(Path::<Arity256>::try_from_bytes(&[0, 255]).is_some());
    }

    #[test]
    fn as_bytes_round_trips() {
        let bytes = [1u8, 2, 3, 14];
        let p = Path::<Arity16>::try_from_bytes(&bytes).expect("in range");
        assert_eq!(p.as_bytes(), &bytes);
        let p = Path::<Arity256>::try_from_bytes(&[0, 200, 255]).expect("in range");
        assert_eq!(p.as_bytes(), &[0, 200, 255]);
    }

    #[test]
    fn conversions_from_slice_vec_and_iterator_agree() {
        let idx = [u4(1), u4(2)];
        let a = Path::<Arity16>::from(&idx[..]);
        let b = Path::<Arity16>::from(vec![u4(1), u4(2)]);
        let c: Path<Arity16> = idx.iter().copied().collect();
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a.as_ref(), &idx[..]);
    }

    #[test]
    fn ordering_is_lexicographic() {
        let p = |b: &[u8]| Path::<Arity16>::try_from_bytes(b).expect("in range");
        assert!(p(&[1]) < p(&[1, 0]));
        assert!(p(&[1, 0]) < p(&[2]));
        assert!(p(&[]) < p(&[0]));
        let mut v = vec![p(&[2]), p(&[1, 0]), p(&[1]), p(&[])];
        v.sort();
        assert_eq!(v, vec![p(&[]), p(&[1]), p(&[1, 0]), p(&[2])]);
    }

    #[test]
    fn debug_prints_decimal_indices() {
        let p = Path::<Arity16>::try_from_bytes(&[10, 0, 15]).expect("in range");
        assert_eq!(alloc::format!("{p:?}"), "[10, 0, 15]");
    }

    #[test]
    fn join_concatenates_head_index_tail() {
        let head = [u4(1), u4(2)];
        let tail = [u4(4)];
        let joined = join::<Arity16>(&head, u4(3), &tail);
        assert_eq!(&*joined, &[u4(1), u4(2), u4(3), u4(4)]);
        let joined = join::<Arity16>(&[], u4(7), &[]);
        assert_eq!(&*joined, &[u4(7)]);
    }

    #[test]
    fn common_prefix_splits_shared_and_unique_parts() {
        let a = [1u8, 2, 3, 4];
        let b = [1u8, 2, 9];
        let o = common_prefix(&a, &b);
        assert_eq!(o.shared, &[1, 2]);
        assert_eq!(o.unique_a, &[3, 4]);
        assert_eq!(o.unique_b, &[9]);

        let o = common_prefix(&a, &a);
        assert_eq!(o.shared, &a);
        assert_eq!((o.unique_a, o.unique_b), (&[][..], &[][..]));

        let o = common_prefix(&a[..2], &a);
        assert_eq!(o.shared, &[1, 2]);
        assert_eq!(o.unique_a, &[] as &[u8]);
        assert_eq!(o.unique_b, &[3, 4]);

        let empty: [u8; 0] = [];
        let o = common_prefix(&empty, &b);
        assert_eq!((o.shared, o.unique_a), (&[][..], &[][..]));
        assert_eq!(o.unique_b, &b);
    }

    #[test]
    fn hash_agrees_with_equality() {
        use core::hash::Hash;
        use core::hash::Hasher;
        fn h<T: Hash>(t: &T) -> u64 {
            struct Fnv(u64);
            impl Hasher for Fnv {
                fn finish(&self) -> u64 {
                    self.0
                }
                fn write(&mut self, bytes: &[u8]) {
                    for b in bytes {
                        self.0 ^= u64::from(*b);
                        self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
                    }
                }
            }
            let mut f = Fnv(0xcbf2_9ce4_8422_2325);
            t.hash(&mut f);
            f.finish()
        }
        let a = Path::<Arity256>::try_from_bytes(&[1, 2]).expect("in range");
        let b = Path::<Arity256>::from(vec![1u8, 2]);
        assert_eq!(h(&a), h(&b));
        let both: Vec<Path<Arity256>> = vec![a, b];
        assert_eq!(both[0], both[1]);
    }
}
