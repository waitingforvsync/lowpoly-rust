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

/// A document material, referenced by faces. Material 0 is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialId(pub u32);
