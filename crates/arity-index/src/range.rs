//! Double-ended range iterators over [`Niche`](crate::Niche) values.

use crate::Niche;
use core::marker::PhantomData;

/// Placeholder — implemented in a later task.
pub struct NicheRange<N: Niche>(PhantomData<N>);

/// Placeholder — implemented in a later task.
pub struct NicheRangeInclusive<N: Niche>(PhantomData<N>);
