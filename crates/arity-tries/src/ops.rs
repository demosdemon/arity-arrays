//! Lookup and mutation over a store.

use core::ptr;

use arity_arrays::Arity;

use crate::Node;
use crate::chain::ChainStack;
use crate::children::ChildMap;
use crate::children::ChildStore;
use crate::path::common_prefix;
use crate::store::EdgeStore;
use crate::store::NodeRef;

/// The node whose full path equals `key`, valued or not.
///
/// A loop over a handle chain, so a key of any length is looked up on a
/// bounded stack.
///
/// # Errors
///
/// The store's error from any [`read`](EdgeStore::read) along the path.
#[expect(
    clippy::type_complexity,
    reason = "the layering of the result is the API: a store error, then found or not, then the \
              node; an alias would hide it from the reader"
)]
pub fn get_node<'a, V, A, S, St>(
    root: Option<&'a Node<V, St::Edge, A, S>>,
    store: &'a St,
    key: &[A::Index],
) -> Result<Option<NodeRef<'a, V, A, S, St>>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let Some(root) = root else {
        return Ok(None);
    };
    let mut chain = ChainStack::new();
    let mut cur: *const Node<V, St::Edge, A, S> = ptr::from_ref(root);
    let mut rest = key;
    loop {
        // SAFETY: `cur` is the root, borrowed for `'a`, or the pointee of the
        // chain's last handle, which stays in place until that handle is
        // popped, and nothing pops while `cur` is in use.
        let node = unsafe { &*cur };
        let o = common_prefix(rest, node.partial_path());
        if !o.unique_b.is_empty() {
            return Ok(None);
        }
        let Some((&i, key_rest)) = o.unique_a.split_first() else {
            return Ok(Some(NodeRef::new(chain, cur)));
        };
        let Some(edge) = node.children().get(i) else {
            return Ok(None);
        };
        let handle = store.read(edge)?;
        cur = chain.push(handle);
        rest = key_rest;
    }
}

/// The value at `key`, cloned out. Uses the same handle chain as
/// [`get_node`]: each handle borrows the node its predecessor dereferences
/// to, so no predecessor can be released early.
///
/// # Errors
///
/// The store's error from any [`read`](EdgeStore::read) along the path.
pub fn get<V: Clone, A, S, St>(
    root: Option<&Node<V, St::Edge, A, S>>,
    store: &St,
    key: &[A::Index],
) -> Result<Option<V>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    Ok(get_node(root, store, key)?.and_then(|node| node.value().cloned()))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use arity_arrays::Arity16;
    use arity_arrays::PackedArray;
    use arity_arrays::index::U4;

    use super::*;
    use crate::InMemory;
    use crate::MemEdge;
    use crate::Node;
    use crate::Packed;
    use crate::Path;
    use crate::store::EdgeStore;

    type N = Node<u32, MemEdge<u32, Arity16, Packed, u64>, Arity16, Packed>;
    type St = InMemory<u64>;

    fn p(bytes: &[u8]) -> Path<Arity16> {
        Path::try_from_bytes(bytes).expect("in range")
    }

    /// root [1,2] (no value) -> 3: leaf [4] = 34, 5: node [] = 5 -> 6: leaf []
    /// = 56
    fn sample() -> (N, St) {
        let mut store = St::default();
        let mut mid = N::new(Path::new(), Some(5), PackedArray::default());
        mid.children_mut()
            .insert(U4::new_masked(6), store.inline(N::leaf(Path::new(), 56)));
        let mut root = N::new(p(&[1, 2]), None, PackedArray::default());
        root.children_mut()
            .insert(U4::new_masked(3), store.inline(N::leaf(p(&[4]), 34)));
        root.children_mut()
            .insert(U4::new_masked(5), store.inline(mid));
        (root, store)
    }

    /// `(found, value)`.
    fn lookup(root: &N, store: &St, key: &[u8]) -> (bool, Option<u32>) {
        get_node(Some(root), store, &p(key))
            .expect("infallible")
            .map_or((false, None), |n| (true, n.value().copied()))
    }

    #[test]
    fn get_node_finds_valued_and_valueless_nodes() {
        let (root, store) = sample();
        assert_eq!(lookup(&root, &store, &[1, 2]), (true, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 3, 4]), (true, Some(34)));
        assert_eq!(lookup(&root, &store, &[1, 2, 5]), (true, Some(5)));
        assert_eq!(lookup(&root, &store, &[1, 2, 5, 6]), (true, Some(56)));
    }

    #[test]
    fn get_node_misses_inside_a_partial_path_and_past_a_leaf() {
        let (root, store) = sample();
        assert_eq!(lookup(&root, &store, &[1]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 9]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 3]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 3, 4, 0]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 7]), (false, None));
        assert_eq!(lookup(&root, &store, &[]), (false, None));
    }

    #[test]
    fn get_on_an_empty_trie_is_none() {
        let store = St::default();
        assert_eq!(get(None::<&N>, &store, &p(&[1])).expect("infallible"), None);
    }

    #[test]
    fn get_clones_the_value() {
        let (root, store) = sample();
        assert_eq!(
            get(Some(&root), &store, &p(&[1, 2, 5, 6])).expect("infallible"),
            Some(56)
        );
        assert_eq!(
            get(Some(&root), &store, &p(&[1, 2])).expect("infallible"),
            None
        );
    }

    #[test]
    fn lookup_of_a_deep_chain_does_not_recurse() {
        let depth = if cfg!(miri) { 256 } else { 100_000 };
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let mut store = St::default();
                let mut node = N::leaf(Path::new(), 7);
                for _ in 0..depth {
                    let mut parent = N::new(Path::new(), None, PackedArray::default());
                    parent
                        .children_mut()
                        .insert(U4::new_masked(0), store.inline(node));
                    node = parent;
                }
                let key = alloc::vec![U4::new_masked(0); depth];
                let found = get(Some(&node), &store, &key).expect("infallible");
                drop(node);
                found
            })
            .expect("spawn");
        assert_eq!(handle.join().expect("no overflow"), Some(7));
    }
}
