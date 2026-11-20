//! The editable mesh type, its storage, and its queries. See the [`mesh`](super) module
//! documentation for the structure.

use glam::{DVec2, DVec3};

use super::{EdgeId, FaceId, HalfEdgeId, LoopId, MaterialId, VertId};
use crate::cow_vec::{CowVec, chunk_len};
use crate::math::newell_normal;

/// How shading treats an edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Shading {
    /// Normals split across the edge, a crease. The default, for the faceted low-poly look.
    #[default]
    Hard,
    /// Normals are smoothed across the edge.
    Soft,
}

/// A face corner: a vertex and the UV the face uses there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corner {
    /// The corner's vertex.
    pub v: VertId,
    /// The corner's UV, in world units.
    pub uv: DVec2,
}

impl Corner {
    /// A corner at `v` with UV (0, 0).
    pub fn at(v: VertId) -> Self {
        Self { v, uv: DVec2::ZERO }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct VertRecord {
    pub(super) first_out: Option<HalfEdgeId>,
    pub(super) alive: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HalfEdgeRecord {
    pub(super) origin: VertId,
    pub(super) next: Option<HalfEdgeId>,
    pub(super) prev: Option<HalfEdgeId>,
    pub(super) loop_id: Option<LoopId>,
    /// The next outgoing half-edge of `origin`, in its unordered out list.
    pub(super) next_out: Option<HalfEdgeId>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EdgeRecord {
    pub(super) alive: bool,
    pub(super) shading: Shading,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct LoopRecord {
    pub(super) face: FaceId,
    pub(super) first: HalfEdgeId,
    /// The next hole of the face. Chained from the outer loop.
    pub(super) next_hole: Option<LoopId>,
    pub(super) alive: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FaceRecord {
    pub(super) outer: LoopId,
    pub(super) material: MaterialId,
    pub(super) alive: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LiveCounts {
    pub(super) verts: usize,
    pub(super) edges: usize,
    pub(super) faces: usize,
}

// Chunk lengths sized to about 4 KB per chunk for each element type.
const POSITION_CHUNK: usize = chunk_len(size_of::<DVec3>());
const VERT_CHUNK: usize = chunk_len(size_of::<VertRecord>());
const HALF_EDGE_CHUNK: usize = chunk_len(size_of::<HalfEdgeRecord>());
const UV_CHUNK: usize = chunk_len(size_of::<DVec2>());
const EDGE_CHUNK: usize = chunk_len(size_of::<EdgeRecord>());
const LOOP_CHUNK: usize = chunk_len(size_of::<LoopRecord>());
const FACE_CHUNK: usize = chunk_len(size_of::<FaceRecord>());

/// An editable polygon mesh with n-gon faces, holes, per-corner UVs and wire edges.
///
/// See the [module documentation](super) for the structure.
///
/// Queries taking a handle expect a live element of this mesh. A dead handle returns
/// unspecified (but memory-safe) values, and debug builds assert; an out-of-range handle panics.
/// Code that holds handles across edits, such as a selection carried over an undo, checks them
/// with the `contains_*` methods. Fallible operations check their inputs and return
/// [`MeshError`](super::MeshError) instead, leaving the mesh unchanged.
#[derive(Clone, Debug, Default)]
pub struct EditableMesh {
    pub(super) positions: CowVec<DVec3, POSITION_CHUNK>,
    pub(super) verts: CowVec<VertRecord, VERT_CHUNK>,
    pub(super) half_edges: CowVec<HalfEdgeRecord, HALF_EDGE_CHUNK>,
    pub(super) uvs: CowVec<DVec2, UV_CHUNK>,
    pub(super) edges: CowVec<EdgeRecord, EDGE_CHUNK>,
    pub(super) loops: CowVec<LoopRecord, LOOP_CHUNK>,
    pub(super) faces: CowVec<FaceRecord, FACE_CHUNK>,
    pub(super) live: LiveCounts,
}

// Meshes are shared with background work (autosave) and between undo states.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EditableMesh>();
};

/// Indices of the records for which `alive` holds.
fn live_slots<R: Copy, const N: usize>(
    records: &CowVec<R, N>,
    alive: fn(&R) -> bool,
) -> impl Iterator<Item = usize> + '_ {
    records
        .iter()
        .enumerate()
        .filter_map(move |(i, r)| alive(r).then_some(i))
}

impl EditableMesh {
    /// Creates an empty mesh.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live vertices.
    pub fn vert_count(&self) -> usize {
        self.live.verts
    }

    /// Number of live edges.
    pub fn edge_count(&self) -> usize {
        self.live.edges
    }

    /// Number of live faces.
    pub fn face_count(&self) -> usize {
        self.live.faces
    }

    /// Number of vertex slots, live or dead: one more than the largest vertex index ever used.
    pub fn vert_slots(&self) -> usize {
        self.verts.len()
    }

    /// Number of face slots, live or dead: one more than the largest face index ever used.
    pub fn face_slots(&self) -> usize {
        self.faces.len()
    }

    /// Live vertices, in index order.
    pub fn verts(&self) -> impl Iterator<Item = VertId> + '_ {
        live_slots(&self.verts, |r| r.alive).map(VertId::from_slot)
    }

    /// Live edges, in index order.
    pub fn edges(&self) -> impl Iterator<Item = EdgeId> + '_ {
        live_slots(&self.edges, |r| r.alive).map(EdgeId::from_slot)
    }

    /// Live faces, in index order.
    pub fn faces(&self) -> impl Iterator<Item = FaceId> + '_ {
        live_slots(&self.faces, |r| r.alive).map(FaceId::from_slot)
    }

    /// Whether `v` is a live vertex of this mesh.
    pub fn contains_vert(&self, v: VertId) -> bool {
        self.verts.get(v.slot()).is_some_and(|r| r.alive)
    }

    /// Whether `e` is a live edge of this mesh.
    pub fn contains_edge(&self, e: EdgeId) -> bool {
        self.edges.get(e.slot()).is_some_and(|r| r.alive)
    }

    /// Whether `h` is a half-edge of a live edge of this mesh.
    pub fn contains_half_edge(&self, h: HalfEdgeId) -> bool {
        self.contains_edge(h.edge())
    }

    /// Whether `f` is a live face of this mesh.
    pub fn contains_face(&self, f: FaceId) -> bool {
        self.faces.get(f.slot()).is_some_and(|r| r.alive)
    }

    /// Whether `l` is a live loop of this mesh.
    pub fn contains_loop(&self, l: LoopId) -> bool {
        self.loops.get(l.slot()).is_some_and(|r| r.alive)
    }

    // ---- vertices ----

    /// Position of a vertex.
    pub fn position(&self, v: VertId) -> DVec3 {
        debug_assert!(self.contains_vert(v), "dead or unknown vertex {v}");
        self.positions[v.slot()]
    }

    /// Moves a vertex. Callers keep faces planar (see `planarity`).
    pub fn set_position(&mut self, v: VertId, p: DVec3) {
        debug_assert!(self.contains_vert(v), "dead or unknown vertex {v}");
        self.positions[v.slot()] = p;
    }

    /// Outgoing half-edges of a vertex, in no particular order.
    pub fn outgoing(&self, v: VertId) -> impl Iterator<Item = HalfEdgeId> + Clone + '_ {
        debug_assert!(self.contains_vert(v), "dead or unknown vertex {v}");
        std::iter::successors(self.verts[v.slot()].first_out, |h| {
            self.half_edges[h.slot()].next_out
        })
    }

    /// Number of edges at a vertex.
    pub fn valence(&self, v: VertId) -> usize {
        self.outgoing(v).count()
    }

    // ---- half-edges ----

    /// The vertex a half-edge starts from.
    pub fn origin(&self, h: HalfEdgeId) -> VertId {
        debug_assert!(self.contains_half_edge(h), "dead or unknown half-edge {h}");
        self.half_edges[h.slot()].origin
    }

    /// The vertex a half-edge points to.
    pub fn dest(&self, h: HalfEdgeId) -> VertId {
        self.origin(h.twin())
    }

    /// The next half-edge around its loop, or `None` on the open boundary.
    pub fn next(&self, h: HalfEdgeId) -> Option<HalfEdgeId> {
        debug_assert!(self.contains_half_edge(h), "dead or unknown half-edge {h}");
        self.half_edges[h.slot()].next
    }

    /// The previous half-edge around its loop, or `None` on the open boundary.
    pub fn prev(&self, h: HalfEdgeId) -> Option<HalfEdgeId> {
        debug_assert!(self.contains_half_edge(h), "dead or unknown half-edge {h}");
        self.half_edges[h.slot()].prev
    }

    /// The loop a half-edge belongs to, or `None` on the open boundary.
    pub fn loop_of(&self, h: HalfEdgeId) -> Option<LoopId> {
        debug_assert!(self.contains_half_edge(h), "dead or unknown half-edge {h}");
        self.half_edges[h.slot()].loop_id
    }

    /// The face a half-edge belongs to, or `None` on the open boundary.
    pub fn face_of(&self, h: HalfEdgeId) -> Option<FaceId> {
        self.loop_of(h).map(|l| self.loops[l.slot()].face)
    }

    /// The UV of the face corner at the half-edge's origin. (0, 0) on the open boundary.
    pub fn uv(&self, h: HalfEdgeId) -> DVec2 {
        debug_assert!(self.contains_half_edge(h), "dead or unknown half-edge {h}");
        self.uvs[h.slot()]
    }

    /// Sets the UV of the face corner at the half-edge's origin. The half-edge must be on a
    /// face; boundary half-edges have no corner.
    pub fn set_uv(&mut self, h: HalfEdgeId, uv: DVec2) {
        debug_assert!(
            self.loop_of(h).is_some(),
            "half-edge {h} has no face corner"
        );
        self.uvs[h.slot()] = uv;
    }

    // ---- edges ----

    /// The edge's endpoints, in the direction of its primary half-edge.
    pub fn edge_verts(&self, e: EdgeId) -> [VertId; 2] {
        let h = e.half_edge();
        [self.origin(h), self.dest(h)]
    }

    /// Faces on either side of an edge (0, 1 or 2).
    pub fn edge_faces(&self, e: EdgeId) -> impl Iterator<Item = FaceId> + use<> {
        let h = e.half_edge();
        let a = self.face_of(h);
        let b = self.face_of(h.twin()).filter(|b| Some(*b) != a);
        a.into_iter().chain(b)
    }

    /// Whether neither side of the edge has a face.
    pub fn is_wire(&self, e: EdgeId) -> bool {
        let h = e.half_edge();
        self.loop_of(h).is_none() && self.loop_of(h.twin()).is_none()
    }

    /// Whether exactly one side of the edge has a face.
    pub fn is_boundary(&self, e: EdgeId) -> bool {
        let h = e.half_edge();
        self.loop_of(h).is_none() != self.loop_of(h.twin()).is_none()
    }

    /// The edge's shading.
    pub fn shading(&self, e: EdgeId) -> Shading {
        debug_assert!(self.contains_edge(e), "dead or unknown edge {e}");
        self.edges[e.slot()].shading
    }

    /// Sets the edge's shading.
    pub fn set_shading(&mut self, e: EdgeId, shading: Shading) {
        debug_assert!(self.contains_edge(e), "dead or unknown edge {e}");
        self.edges[e.slot()].shading = shading;
    }

    /// The edge joining two vertices, if any. O(valence of `a`).
    pub fn find_edge(&self, a: VertId, b: VertId) -> Option<EdgeId> {
        self.find_half_edge(a, b).map(HalfEdgeId::edge)
    }

    /// The half-edge running from `a` to `b`, if the edge exists. O(valence of `a`).
    pub fn find_half_edge(&self, a: VertId, b: VertId) -> Option<HalfEdgeId> {
        self.outgoing(a).find(|&h| self.dest(h) == b)
    }

    // ---- loops ----

    /// The face a loop bounds.
    pub fn loop_face(&self, l: LoopId) -> FaceId {
        debug_assert!(self.contains_loop(l), "dead or unknown loop {l}");
        self.loops[l.slot()].face
    }

    /// Whether a loop is a hole of its face, rather than its outer boundary.
    pub fn is_hole(&self, l: LoopId) -> bool {
        self.face_outer(self.loop_face(l)) != l
    }

    /// The half-edges around a loop, starting from its first.
    pub fn loop_half_edges(&self, l: LoopId) -> impl Iterator<Item = HalfEdgeId> + Clone + '_ {
        debug_assert!(self.contains_loop(l), "dead or unknown loop {l}");
        self.half_edges_from(self.loops[l.slot()].first)
    }

    /// The half-edges around a loop, starting from `start`. A boundary half-edge has no loop, so
    /// this yields just `start`.
    pub fn half_edges_from(
        &self,
        start: HalfEdgeId,
    ) -> impl Iterator<Item = HalfEdgeId> + Clone + '_ {
        std::iter::successors(Some(start), move |&h| self.next(h).filter(|&n| n != start))
    }

    /// The vertices around a loop.
    pub fn loop_verts(&self, l: LoopId) -> impl Iterator<Item = VertId> + Clone + '_ {
        self.loop_half_edges(l).map(|h| self.origin(h))
    }

    /// The positions around a loop.
    pub fn loop_positions(&self, l: LoopId) -> impl Iterator<Item = DVec3> + Clone + '_ {
        self.loop_verts(l).map(|v| self.position(v))
    }

    /// The corners (vertex and UV) around a loop.
    pub fn loop_corners(&self, l: LoopId) -> impl Iterator<Item = Corner> + Clone + '_ {
        self.loop_half_edges(l).map(|h| self.corner(h))
    }

    /// The face corner at a half-edge's origin.
    pub fn corner(&self, h: HalfEdgeId) -> Corner {
        Corner {
            v: self.origin(h),
            uv: self.uv(h),
        }
    }

    // ---- faces ----

    /// A face's outer loop.
    pub fn face_outer(&self, f: FaceId) -> LoopId {
        debug_assert!(self.contains_face(f), "dead or unknown face {f}");
        self.faces[f.slot()].outer
    }

    /// A face's hole loops, in order.
    pub fn face_holes(&self, f: FaceId) -> impl Iterator<Item = LoopId> + Clone + '_ {
        std::iter::successors(self.loops[self.face_outer(f).slot()].next_hole, |l| {
            self.loops[l.slot()].next_hole
        })
    }

    /// A face's loops: the outer loop, then its holes.
    pub fn face_loops(&self, f: FaceId) -> impl Iterator<Item = LoopId> + Clone + '_ {
        std::iter::once(self.face_outer(f)).chain(self.face_holes(f))
    }

    /// Every half-edge of a face, over all its loops.
    pub fn face_half_edges(&self, f: FaceId) -> impl Iterator<Item = HalfEdgeId> + Clone + '_ {
        self.face_loops(f).flat_map(|l| self.loop_half_edges(l))
    }

    /// Every vertex position of a face, over all its loops.
    pub fn face_positions(&self, f: FaceId) -> impl Iterator<Item = DVec3> + Clone + '_ {
        self.face_half_edges(f)
            .map(|h| self.position(self.origin(h)))
    }

    /// An owned snapshot of a face's corners, one list per loop (outer first, then holes), for
    /// rebuilding the face. Allocates; use the iterator queries for reading.
    pub fn face_corners(&self, f: FaceId) -> Vec<Vec<Corner>> {
        self.face_loops(f)
            .map(|l| self.loop_corners(l).collect())
            .collect()
    }

    /// A face's material.
    pub fn material(&self, f: FaceId) -> MaterialId {
        debug_assert!(self.contains_face(f), "dead or unknown face {f}");
        self.faces[f.slot()].material
    }

    /// Sets a face's material.
    pub fn set_material(&mut self, f: FaceId, material: MaterialId) {
        debug_assert!(self.contains_face(f), "dead or unknown face {f}");
        self.faces[f.slot()].material = material;
    }

    /// Unit normal of a face, from its outer loop (Newell's method). Zero for a degenerate face.
    pub fn face_normal(&self, f: FaceId) -> DVec3 {
        newell_normal(self.loop_positions(self.face_outer(f))).normalize_or_zero()
    }
}
