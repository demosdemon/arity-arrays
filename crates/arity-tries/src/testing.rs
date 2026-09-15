//! Test support shared by the unit tests: a store that fails on demand.

use core::cell::Cell;

use arity_arrays::Arity16;

use crate::InMemory;
use crate::MemEdge;
use crate::Node;
use crate::Packed;
use crate::store::EdgeStore;

type N = Node<u32, MemEdge<u32, Arity16, Packed, u64>, Arity16, Packed>;
type St = InMemory<u64>;

/// Wraps [`InMemory`] and fails `read` on the `fail_read_at`th read and
/// `materialize` on the `fail_materialize_at`th materialize (1-based; `0`
/// never fails).
pub struct Failing {
    pub inner: St,
    reads: Cell<usize>,
    fail_read_at: usize,
    materializes: usize,
    fail_materialize_at: usize,
}

impl Failing {
    pub fn new(inner: St, fail_read_at: usize, fail_materialize_at: usize) -> Self {
        Self {
            inner,
            reads: Cell::new(0),
            fail_read_at,
            materializes: 0,
            fail_materialize_at,
        }
    }
}

impl EdgeStore<u32, Arity16, Packed> for Failing {
    type Edge = MemEdge<u32, Arity16, Packed, u64>;
    type Hash = u64;
    type Error = &'static str;
    type Shared<'e>
        = &'e N
    where
        Self: 'e;

    fn read<'e>(&'e self, edge: &'e Self::Edge) -> Result<&'e N, &'static str> {
        self.reads.set(self.reads.get() + 1);
        if self.reads.get() == self.fail_read_at {
            return Err("read");
        }
        self.inner.read(edge).map_err(|e| match e {})
    }

    fn as_inline(edge: &mut Self::Edge) -> Option<&mut N> {
        St::as_inline(edge)
    }

    fn materialize<'e>(&mut self, edge: &'e mut Self::Edge) -> Result<&'e mut N, &'static str> {
        self.materializes += 1;
        if self.materializes == self.fail_materialize_at {
            return Err("materialize");
        }
        self.inner.materialize(edge).map_err(|e| match e {})
    }

    fn inline(&mut self, node: N) -> Self::Edge {
        self.inner.inline(node)
    }

    fn seal(&mut self, edge: &mut Self::Edge, hash: u64) {
        self.inner.seal(edge, hash);
    }

    fn hash(edge: &Self::Edge) -> Option<&u64> {
        St::hash(edge)
    }
}
