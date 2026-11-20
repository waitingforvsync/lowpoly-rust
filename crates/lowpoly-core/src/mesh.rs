//! The editable mesh: an index-based half-edge structure with structural sharing.
//!
//! # Structure
//!
//! - Every undirected edge owns two half-edges pointing in opposite directions. They are
//!   allocated as a pair, so a half-edge's twin is `index ^ 1` and its edge is `index / 2`
//!   ([`HalfEdgeId::twin`], [`HalfEdgeId::edge`], [`EdgeId::half_edge`]).
//! - A half-edge on a face loop has a loop, `next` and `prev`. A half-edge on the open boundary
//!   (or either half of a wire edge) has no loop, no `next`/`prev`, and a zero UV.
//! - A face owns an outer loop (counter-clockwise about its normal) and any number of hole loops
//!   (clockwise), chained from the outer loop.
//! - Each vertex has an unordered list of outgoing half-edges, threaded through the half-edges
//!   (`first_out` on the vertex, `next_out` on each half-edge). This avoids relinking boundary
//!   cycles, and supports wire edges and vertices where separate fans meet.
//! - Edges are manifold: each half-edge belongs to at most one face, so an edge borders at most
//!   two faces and neighbouring faces always wind consistently.
//! - A UV lives on the half-edge: it is the UV of the face corner at the half-edge's origin.
//!
//! # Storage and handles
//!
//! Each attribute is a separate copy-on-write chunked array, so cloning a mesh is O(1) and an
//! edit copies only the chunks it touches. Storage is append-only: removal marks an element dead,
//! and new elements are appended, so a handle never comes to mean a different element. The one
//! exception is deliberate: when an operator rebuilds a face it reuses the face's slot, so the
//! face keeps its id (its loops are new, so loop ids from before the rebuild go stale).
//!
//! # Files
//!
//! - `handles`: the typed element indices.
//! - `editable_mesh`: [`EditableMesh`], its storage records and its queries.
//! - `elements`: adding and removing vertices, edges and faces; transactions.
//! - `euler`: topological operators (split, join, holes).
//! - `validate`: invariant checks.

mod editable_mesh;
mod elements;
mod error;
mod euler;
mod handles;
mod validate;

pub use editable_mesh::{Corner, EditableMesh, Shading};
pub use elements::RemoveFace;
pub use error::MeshError;
pub use euler::{AddedHole, HoleFill};
pub use handles::{EdgeId, FaceId, HalfEdgeId, LoopId, MaterialId, VertId};
