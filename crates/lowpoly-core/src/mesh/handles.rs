//! Handles: typed indices of mesh elements.

use std::fmt;
use std::num::NonZeroU32;

macro_rules! handle {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        ///
        /// Handles are indices into the mesh's storage. `Option` of a handle is the same size as
        /// the handle.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// The handle for the element at `index`.
            ///
            /// # Panics
            ///
            /// Panics if `index` is `u32::MAX`.
            pub const fn new(index: u32) -> Self {
                match NonZeroU32::new(index.wrapping_add(1)) {
                    Some(n) => Self(n),
                    None => panic!("handle index out of range"),
                }
            }

            /// The element's index in the mesh's storage.
            pub const fn index(self) -> u32 {
                self.0.get() - 1
            }

            pub(super) fn slot(self) -> usize {
                self.index() as usize
            }

            pub(super) fn from_slot(slot: usize) -> Self {
                Self::new(u32::try_from(slot).expect("meshes have fewer than 2^32 elements of a kind"))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.index())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.index())
            }
        }
    };
}

handle!(
    /// A vertex of a [`EditableMesh`](super::EditableMesh).
    VertId,
    "v"
);
handle!(
    /// A half-edge of a [`EditableMesh`](super::EditableMesh).
    HalfEdgeId,
    "h"
);
handle!(
    /// An undirected edge of a [`EditableMesh`](super::EditableMesh).
    EdgeId,
    "e"
);
handle!(
    /// A boundary loop (outer or hole) of a face of a [`EditableMesh`](super::EditableMesh).
    LoopId,
    "l"
);
handle!(
    /// A face of a [`EditableMesh`](super::EditableMesh).
    FaceId,
    "f"
);

// Half-edges are allocated in pairs, one pair per edge, so these relationships are index
// arithmetic and need no mesh.

impl HalfEdgeId {
    /// The opposite half-edge of the same edge.
    #[must_use]
    pub const fn twin(self) -> Self {
        Self::new(self.index() ^ 1)
    }

    /// The edge this half-edge belongs to.
    #[must_use]
    pub const fn edge(self) -> EdgeId {
        EdgeId::new(self.index() / 2)
    }
}

impl EdgeId {
    /// The edge's primary half-edge: the one created first, running from the edge's first
    /// vertex to its second.
    ///
    /// # Panics
    ///
    /// Panics if the index is 2^31 or more, which no live edge can have (half-edges are `u32`
    /// indices, two per edge).
    #[must_use]
    pub const fn half_edge(self) -> HalfEdgeId {
        match self.index().checked_mul(2) {
            Some(index) => HalfEdgeId::new(index),
            None => panic!("edge index too large to have half-edges"),
        }
    }
}

/// A document material, referenced by faces. Material 0 is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialId(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_of_a_handle_is_four_bytes() {
        assert_eq!(size_of::<Option<HalfEdgeId>>(), 4);
    }

    #[test]
    fn twin_edge_and_half_edge_agree() {
        let e = EdgeId::new(7);
        let h = e.half_edge();
        assert_eq!(h.index(), 14);
        assert_eq!(h.twin().index(), 15);
        assert_eq!(h.twin().twin(), h);
        assert_eq!(h.edge(), e);
        assert_eq!(h.twin().edge(), e);
    }

    #[test]
    #[should_panic(expected = "too large")]
    fn half_edge_of_an_impossible_edge_panics_instead_of_wrapping() {
        let _ = EdgeId::new(0x8000_0000).half_edge();
    }
}
