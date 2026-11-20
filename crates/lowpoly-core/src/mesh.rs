//! The editable mesh: an index-based half-edge structure with structural sharing.
//!
//! # Structure
//!
//! - Every undirected edge owns two half-edges pointing in opposite directions. They are
//!   allocated as a pair, so a half-edge's twin is `index ^ 1` and its edge is `index / 2`.
//! - A half-edge on a face loop has a loop, `next` and `prev`. A half-edge on the open boundary
//!   (or either half of a wire edge) has no loop and no `next`/`prev`.
//! - A face owns an outer loop (counter-clockwise about its normal) and any number of hole loops
//!   (clockwise), chained from the outer loop.
//! - Each vertex has an unordered list of outgoing half-edges, threaded through the half-edges
//!   (`first_out` on the vertex, `next_out` on each half-edge). This avoids relinking boundary
//!   cycles, and supports wire edges and vertices where separate fans meet.
//! - Edges are manifold: each half-edge belongs to at most one face, so an edge borders at most
//!   two faces and neighbouring faces always wind consistently.
//! - A UV lives on the half-edge: it is the UV of the face corner at the half-edge's origin.
//!
//! # Storage
//!
//! Each attribute is a separate [`CowVec`](crate::cow_vec::CowVec), so cloning a mesh is O(1)
//! and an edit copies only the chunks it touches. Elements are never moved or reused: removal
//! marks them dead, and handles stay valid identifiers for as long as the mesh value exists. New
//! elements are appended.
//!
//! # Files
//!
//! - `handles`: the typed element indices.
//! - `editable_mesh`: [`EditableMesh`], its storage records and its queries.
//! - `primitives`: adding and removing vertices, edges and faces; transactions.
//! - `euler`: topological operators (split, join, holes).
//! - `validate`: invariant checks.

mod editable_mesh;
mod error;
mod euler;
mod handles;
mod primitives;
mod validate;

pub use editable_mesh::{Corner, EditableMesh, Shading};
pub use error::MeshError;
pub use euler::{AddedHole, HoleFill};
pub use handles::{EdgeId, FaceId, HalfEdgeId, LoopId, MaterialId, VertId};
pub use primitives::RemoveFace;
