//! The raw-pointer frame stack behind mutation and hashing.
//!
//! # Invariant
//!
//! A [`Frames`] stack holds `*mut N` pointers to the nodes on the current path
//! from the root down, each derived from a `&mut N`: the root's own borrow, or
//! the reference the store returned for an edge borrowed from the node above
//! it. The whole walk holds `&mut` on the
//! root, so no other access to the trie exists for its duration. The
//! invariant is **strict last-in, first-out use: once any access, read or
//! write, goes through a frame's pointer, every deeper frame is discarded and
//! never used again.** This is exactly what Stacked Borrows, the aliasing model
//! Miri checks raw pointers against, requires: reborrowing through an ancestor
//! pops the tags of every pointer derived beneath it, so an algorithm that
//! reads a parent's map while it still intends to use a child pointer is
//! undefined behavior.
//!
//! The API enforces the invariant by construction rather than by discipline:
//! the only node reachable is the top frame's, a child frame can only be
//! pushed by deriving it from the top, and the only way back to a parent is to
//! pop the frames above it. Each frame also carries plain data of type `X`
//! (a child index, a bitmap of children still to visit), which any frame may
//! expose freely because reading it touches no node.
//!
//! A consequence is that a node's children map can change shape only through
//! that node's own pointer with no deeper frame live, which is also what keeps
//! every held pointer's address stable for edges that hold their node by
//! value.

use alloc::vec::Vec;
use core::marker::PhantomData;
use core::ptr;

/// A path of node pointers from the root down, plus per-frame data.
pub struct Frames<'r, N, X> {
    frames: Vec<(*mut N, X)>,
    _root: PhantomData<&'r mut N>,
}

impl<'r, N, X> Frames<'r, N, X> {
    /// A stack whose only frame is `root`.
    pub fn new(root: &'r mut N, extra: X) -> Self {
        Self {
            frames: alloc::vec![(ptr::from_mut(root), extra)],
            _root: PhantomData,
        }
    }

    /// The top frame's node and data. This is the only standing reference to
    /// a node the stack hands out ([`try_push_child`](Self::try_push_child)
    /// exposes the top node only to the closure that derives its child), which
    /// is what makes it sound.
    pub fn top(&mut self) -> (&mut N, &mut X) {
        let (node, extra) = self.frames.last_mut().expect("a stack is never empty");
        // SAFETY: `node` was derived from a `&mut N` (the root's borrow, or
        // the reference `derive` returned for an edge of the frame below) and
        // every access since has gone through this frame or a frame above it
        // that has since been popped, so no reborrow through an ancestor has
        // invalidated it. The root borrow `'r` outlives `self`, and the edge
        // borrows that produced deeper frames are kept live by the same
        // discipline: nothing reaches an ancestor while this frame exists.
        (unsafe { &mut **node }, extra)
    }

    /// The top frame's data alone.
    pub fn top_extra(&mut self) -> &mut X {
        &mut self.frames.last_mut().expect("a stack is never empty").1
    }

    /// Pushes a child of the top node. `derive` receives the top node and
    /// returns the child (an edge's node the store handed out) with the
    /// child's frame data; on `Err` nothing is pushed.
    ///
    /// # Errors
    ///
    /// Whatever `derive` fails with, typically the store's `materialize`.
    pub fn try_push_child<E>(
        &mut self,
        derive: impl FnOnce(&mut N) -> Result<(&mut N, X), E>,
    ) -> Result<(), E> {
        let (top, _) = *self.frames.last().expect("a stack is never empty");
        // SAFETY: as in `top`: the top frame's pointer is valid and nothing
        // above it exists. The child reference `derive` returns is tied to
        // this reborrow, and the frame pushed for it is only used while no
        // access goes through the parent, which the API guarantees.
        let (child, extra) = derive(unsafe { &mut *top })?;
        self.frames.push((ptr::from_mut(child), extra));
        Ok(())
    }

    /// Gives the top node one last time, then discards its frame. Unlike
    /// [`pop`](Self::pop) this also serves the root frame, after which the
    /// stack holds nothing and must not be used.
    pub fn pop_with<R>(&mut self, f: impl FnOnce(&mut N, X) -> R) -> R {
        let (node, extra) = self.frames.pop().expect("a stack is never empty");
        // SAFETY: as in `top`: this frame was the top, so its pointer is
        // valid, and it is used here for the last time.
        f(unsafe { &mut *node }, extra)
    }

    /// Discards the top frame and returns its data; `None` if only the root
    /// frame remains, which is never popped.
    pub fn pop(&mut self) -> Option<X> {
        if self.frames.len() == 1 {
            return None;
        }
        self.frames.pop().map(|(_, extra)| extra)
    }
}
