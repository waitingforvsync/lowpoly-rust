//! Errors from mesh operations.

use super::{EdgeId, FaceId, LoopId, VertId};

/// Why a mesh operation was refused. A refused operation leaves the mesh unchanged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MeshError {
    /// The vertex does not exist (never created, or removed).
    #[error("no vertex {0}")]
    NoSuchVertex(VertId),
    /// The edge does not exist.
    #[error("no edge {0}")]
    NoSuchEdge(EdgeId),
    /// The face does not exist.
    #[error("no face {0}")]
    NoSuchFace(FaceId),
    /// A vertex can only be removed once it has no edges.
    #[error("vertex {vert} still has {count} edge(s)")]
    VertexHasEdges {
        /// The vertex.
        vert: VertId,
        /// How many edges it has.
        count: usize,
    },
    /// An edge needs two distinct vertices.
    #[error("cannot add an edge from vertex {0} to itself")]
    SelfEdge(VertId),
    /// The two vertices are already joined by an edge.
    #[error("edge {0}-{1} already exists")]
    EdgeExists(VertId, VertId),
    /// Only edges with no face on either side can be removed directly.
    #[error("edge {0}-{1} still borders a face")]
    EdgeBordersFace(VertId, VertId),
    /// A face needs at least one loop.
    #[error("a face needs at least one loop")]
    NoLoops,
    /// Every loop of a face needs at least three corners.
    #[error("face loops need at least 3 corners (got {0})")]
    TooFewCorners(usize),
    /// A vertex may appear only once in a face, across all its loops.
    #[error("vertex {0} appears more than once in the face")]
    RepeatedVertex(VertId),
    /// The directed half-edge is already used by another face: the new face would make the
    /// edge non-manifold, or wind opposite to its neighbour.
    #[error(
        "half-edge {from}->{to} is already used by face {face} (non-manifold edge or inconsistent orientation)"
    )]
    HalfEdgeInUse {
        /// Start of the half-edge.
        from: VertId,
        /// End of the half-edge.
        to: VertId,
        /// The face already using it.
        face: FaceId,
    },
    /// Splitting a face needs vertices on its outer loop.
    #[error("both vertices must be on the outer loop of the face")]
    NotOnOuterLoop,
    /// Splitting a face needs two different vertices.
    #[error("vertices must be distinct")]
    SameVertex,
    /// Splitting a face without a path needs vertices that aren't already neighbours.
    #[error("vertices must not be adjacent")]
    AdjacentVertices,
    /// A split path must lie in the face's plane.
    #[error("the path must lie in the plane of the face")]
    PathNotPlanar,
    /// A split would cut through one of the face's holes.
    #[error("the new edge would cross a hole")]
    CrossesHole,
    /// Joining faces needs a face on both sides of the edge.
    #[error("edge must have a face on both sides")]
    NotInteriorEdge,
    /// Joining faces needs two different faces.
    #[error("edge has the same face on both sides")]
    SameFaceBothSides,
    /// Joining faces needs the edge on both faces' outer loops.
    #[error("both sides must be outer loops")]
    NotOuterLoops,
    /// Joining faces needs them to share exactly one edge.
    #[error("faces share {0} edges; expected exactly 1")]
    SharedEdges(usize),
    /// The joined face would visit a vertex twice.
    #[error("faces also touch at a vertex; merging would repeat it")]
    FacesTouch,
    /// Merged or holed faces must stay planar.
    #[error("faces are not coplanar")]
    NotCoplanar,
    /// A hole's vertices must be new to the face.
    #[error("hole vertices must not already be in the face")]
    HoleVertexInFace,
    /// The loop is not one of the face's holes.
    #[error("loop {0} is not a hole of this face")]
    NotAHole(LoopId),
    /// Only an open hole (no face filling it) can be removed.
    #[error("hole is filled by another face; remove or join that face first")]
    HoleFilled,
    /// `validate` found broken invariants.
    #[error("invalid mesh:\n  {}", .0.join("\n  "))]
    Invalid(Vec<String>),
}
