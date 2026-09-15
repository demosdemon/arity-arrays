//! Structural validation.

use core::fmt;

use arity_arrays::Arity;

use crate::Node;
use crate::Path;
use crate::children::ChildMap;
use crate::children::ChildStore;
use crate::iter::Walk;
use crate::store::EdgeStore;

/// A node that breaks the structural invariant, by full path.
pub struct Violation<A: Arity> {
    /// The offending node's full path.
    pub path: Path<A>,
    /// What is wrong with it.
    pub kind: ViolationKind,
}

/// The two ways a node can break the structural invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    /// A node with no value and exactly one child, which `remove` would have
    /// merged with that child.
    ValuelessSingleChild,
    /// A node with no value and no children, which `remove` would have
    /// removed.
    ValuelessLeaf,
}

/// Why [`validate`] failed.
pub enum ValidateError<A: Arity, E> {
    /// The store failed to read an edge.
    Store(E),
    /// The first violation found, in pre-order.
    Invalid(Violation<A>),
}

// Hand-written so that no bound lands on `A`, which is a marker type.
impl<A: Arity> fmt::Debug for Violation<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Violation")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .finish()
    }
}

impl<A: Arity> Clone for Violation<A> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            kind: self.kind,
        }
    }
}

impl<A: Arity> PartialEq for Violation<A> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.kind == other.kind
    }
}

impl<A: Arity> Eq for Violation<A> {}

impl<A: Arity, E: fmt::Debug> fmt::Debug for ValidateError<A, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => f.debug_tuple("Store").field(e).finish(),
            Self::Invalid(v) => f.debug_tuple("Invalid").field(v).finish(),
        }
    }
}

impl<A: Arity, E: Clone> Clone for ValidateError<A, E> {
    fn clone(&self) -> Self {
        match self {
            Self::Store(e) => Self::Store(e.clone()),
            Self::Invalid(v) => Self::Invalid(v.clone()),
        }
    }
}

impl<A: Arity, E: PartialEq> PartialEq for ValidateError<A, E> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Store(a), Self::Store(b)) => a == b,
            (Self::Invalid(a), Self::Invalid(b)) => a == b,
            _ => false,
        }
    }
}

impl<A: Arity, E: Eq> Eq for ValidateError<A, E> {}

impl<A: Arity, E: fmt::Display> fmt::Display for ValidateError<A, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => write!(f, "store error: {e}"),
            Self::Invalid(v) => match v.kind {
                ViolationKind::ValuelessSingleChild => {
                    write!(f, "valueless node with one child at {:?}", v.path)
                }
                ViolationKind::ValuelessLeaf => {
                    write!(f, "valueless node with no children at {:?}", v.path)
                }
            },
        }
    }
}

impl<A: Arity, E: fmt::Debug + fmt::Display> core::error::Error for ValidateError<A, E> {}

/// Checks the structural invariant: a node with no value has at least two
/// children, and a node with no children has a value.
///
/// A read-only walk over every node, valued or not, reporting the first
/// violation in pre-order with the offending node's full path. The invariant
/// is a property of settled tries, not of the type: constructors do not
/// check it, and an adopter's parallel inserter or proof reconstruction may
/// build a valueless single-child root on purpose.
///
/// # Errors
///
/// A store error while reading, or the first violation.
pub fn validate<V, A, S, St>(
    root: Option<&Node<V, St::Edge, A, S>>,
    store: &St,
) -> Result<(), ValidateError<A, St::Error>>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let mut walk = Walk::new(root, store, None);
    loop {
        let (path, node) = match walk.advance() {
            Ok(Some(found)) => found,
            Ok(None) => return Ok(()),
            Err(e) => return Err(ValidateError::Store(e)),
        };
        if node.value().is_some() {
            continue;
        }
        let kind = match node.children().count() {
            0 => ViolationKind::ValuelessLeaf,
            1 => ViolationKind::ValuelessSingleChild,
            _ => continue,
        };
        return Err(ValidateError::Invalid(Violation {
            path: Path::from(path),
            kind,
        }));
    }
}

#[cfg(test)]
mod tests {
    use arity_arrays::Arity16;
    use arity_arrays::PackedArray;
    use arity_arrays::index::U4;

    use super::*;
    use crate::InMemory;
    use crate::MemEdge;
    use crate::Node;
    use crate::Packed;
    use crate::Path;
    use crate::insert;
    use crate::remove;
    use crate::store::EdgeStore;
    use crate::testing::Failing;

    type N = Node<u32, MemEdge<u32, Arity16, Packed, u64>, Arity16, Packed>;
    type St = InMemory<u64>;

    fn p(bytes: &[u8]) -> Path<Arity16> {
        Path::try_from_bytes(bytes).expect("in range")
    }

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    #[test]
    fn settled_tries_are_valid() {
        let (mut root, mut store) = (None::<N>, St::default());
        assert!(validate(root.as_ref(), &store).is_ok());
        for (key, value) in [
            (&[1, 2, 3][..], 3),
            (&[1, 2, 4], 4),
            (&[1, 7], 7),
            (&[8], 8),
        ] {
            insert(&mut root, &mut store, &p(key), value).expect("infallible");
            assert!(validate(root.as_ref(), &store).is_ok());
        }
        for key in [&[1, 2, 3][..], &[8], &[1, 7], &[1, 2, 4]] {
            remove(&mut root, &mut store, &p(key)).expect("infallible");
            assert!(validate(root.as_ref(), &store).is_ok());
        }
        assert!(root.is_none());
    }

    #[test]
    fn a_valueless_single_child_node_is_reported_with_its_full_path() {
        let mut store = St::default();
        let mut mid = N::new(p(&[3]), None, PackedArray::default());
        mid.children_mut()
            .insert(u4(4), store.inline(N::leaf(Path::new(), 1)));
        let mut root = N::new(p(&[1]), Some(0), PackedArray::default());
        root.children_mut().insert(u4(2), store.inline(mid));
        root.children_mut()
            .insert(u4(9), store.inline(N::leaf(Path::new(), 9)));
        let err = validate(Some(&root), &store).expect_err("invalid");
        assert!(matches!(
            err,
            ValidateError::Invalid(Violation { ref path, kind: ViolationKind::ValuelessSingleChild })
                if path.as_bytes() == [1, 2, 3]
        ));
    }

    #[test]
    fn a_valueless_leaf_is_reported() {
        let mut store = St::default();
        let mut root = N::new(Path::new(), None, PackedArray::default());
        root.children_mut()
            .insert(u4(2), store.inline(N::leaf(Path::new(), 1)));
        root.children_mut().insert(
            u4(5),
            store.inline(N::new(p(&[6]), None, PackedArray::default())),
        );
        let err = validate(Some(&root), &store).expect_err("invalid");
        assert!(matches!(
            err,
            ValidateError::Invalid(Violation { ref path, kind: ViolationKind::ValuelessLeaf })
                if path.as_bytes() == [5, 6]
        ));
        let lone = N::new(Path::new(), None, PackedArray::default());
        assert!(matches!(
            validate(Some(&lone), &store),
            Err(ValidateError::Invalid(Violation {
                kind: ViolationKind::ValuelessLeaf,
                ..
            }))
        ));
    }

    #[test]
    fn a_store_error_is_passed_through() {
        let mut store = St::default();
        let mut root = N::new(Path::new(), Some(0), PackedArray::default());
        root.children_mut()
            .insert(u4(2), store.inline(N::leaf(Path::new(), 1)));
        let failing = Failing::new(store, 1, 0);
        assert!(matches!(
            validate(Some(&root), &failing),
            Err(ValidateError::Store("read"))
        ));
    }
}
