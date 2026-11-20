//! The editable mesh type, its storage, and its queries. See the [`mesh`](super) module
//! documentation for the structure.

use glam::{DVec2, DVec3};

use super::{EdgeId, FaceId, HalfEdgeId, LoopId, MaterialId, VertId};
use crate::cow_vec::CowVec;
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

/// An editable polygon mesh with n-gon faces, holes, per-corner UVs and wire edges.
///
/// See the [module documentation](super) for the structure. Queries taking a handle expect a
/// live element of this mesh and may panic otherwise; fallible operations check their inputs and
/// return [`MeshError`](super::MeshError) instead, leaving the mesh unchanged.
#[derive(Clone, Debug, Default)]
pub struct EditableMesh {
    pub(super) positions: CowVec<DVec3>,
    pub(super) verts: CowVec<VertRecord>,
    pub(super) half_edges: CowVec<HalfEdgeRecord>,
    pub(super) uvs: CowVec<DVec2>,
    pub(super) edges: CowVec<EdgeRecord>,
    pub(super) loops: CowVec<LoopRecord>,
    pub(super) faces: CowVec<FaceRecord>,
    pub(super) live: LiveCounts,
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
        self.verts
            .iter()
            .enumerate()
            .filter(|(_, v)| v.alive)
            .map(|(i, _)| VertId::from_slot(i))
    }

    /// Live edges, in index order.
    pub fn edges(&self) -> impl Iterator<Item = EdgeId> + '_ {
        self.edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive)
            .map(|(i, _)| EdgeId::from_slot(i))
    }

    /// Live faces, in index order.
    pub fn faces(&self) -> impl Iterator<Item = FaceId> + '_ {
        self.faces
            .iter()
            .enumerate()
            .filter(|(_, f)| f.alive)
            .map(|(i, _)| FaceId::from_slot(i))
    }

    /// Whether `v` is a live vertex of this mesh.
    pub fn contains_vert(&self, v: VertId) -> bool {
        self.verts.get(v.slot()).is_some_and(|r| r.alive)
    }

    /// Whether `e` is a live edge of this mesh.
    pub fn contains_edge(&self, e: EdgeId) -> bool {
        self.edges.get(e.slot()).is_some_and(|r| r.alive)
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
        self.positions[v.slot()]
    }

    /// Moves a vertex. Callers keep faces planar (see `planarity`).
    pub fn set_position(&mut self, v: VertId, p: DVec3) {
        self.positions[v.slot()] = p;
    }

    /// Outgoing half-edges of a vertex, in no particular order.
    pub fn outgoing(&self, v: VertId) -> impl Iterator<Item = HalfEdgeId> + '_ {
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
        self.half_edges[h.slot()].origin
    }

    /// The vertex a half-edge points to.
    pub fn dest(&self, h: HalfEdgeId) -> VertId {
        self.origin(self.twin(h))
    }

    /// The opposite half-edge of the same edge.
    pub fn twin(&self, h: HalfEdgeId) -> HalfEdgeId {
        HalfEdgeId::new(h.index() ^ 1)
    }

    /// The edge a half-edge belongs to.
    pub fn edge_of(&self, h: HalfEdgeId) -> EdgeId {
        EdgeId::new(h.index() / 2)
    }

    /// The next half-edge around its loop, or `None` on the open boundary.
    pub fn next(&self, h: HalfEdgeId) -> Option<HalfEdgeId> {
        self.half_edges[h.slot()].next
    }

    /// The previous half-edge around its loop, or `None` on the open boundary.
    pub fn prev(&self, h: HalfEdgeId) -> Option<HalfEdgeId> {
        self.half_edges[h.slot()].prev
    }

    /// The loop a half-edge belongs to, or `None` on the open boundary.
    pub fn loop_of(&self, h: HalfEdgeId) -> Option<LoopId> {
        self.half_edges[h.slot()].loop_id
    }

    /// The face a half-edge belongs to, or `None` on the open boundary.
    pub fn face_of(&self, h: HalfEdgeId) -> Option<FaceId> {
        self.loop_of(h).map(|l| self.loops[l.slot()].face)
    }

    /// The UV of the face corner at the half-edge's origin. (0, 0) on the open boundary.
    pub fn uv(&self, h: HalfEdgeId) -> DVec2 {
        self.uvs[h.slot()]
    }

    /// Sets the UV of the face corner at the half-edge's origin.
    pub fn set_uv(&mut self, h: HalfEdgeId, uv: DVec2) {
        self.uvs[h.slot()] = uv;
    }

    // ---- edges ----

    /// The edge's primary half-edge: the one created first, running from its first vertex to
    /// its second.
    pub fn edge_half_edge(&self, e: EdgeId) -> HalfEdgeId {
        HalfEdgeId::new(e.index() * 2)
    }

    /// The edge's endpoints, in the direction of its primary half-edge.
    pub fn edge_verts(&self, e: EdgeId) -> [VertId; 2] {
        let h = self.edge_half_edge(e);
        [self.origin(h), self.dest(h)]
    }

    /// Faces on either side of an edge (0, 1 or 2).
    pub fn edge_faces(&self, e: EdgeId) -> impl Iterator<Item = FaceId> + use<> {
        let h = self.edge_half_edge(e);
        let a = self.face_of(h);
        let b = self.face_of(self.twin(h)).filter(|b| Some(*b) != a);
        a.into_iter().chain(b)
    }

    /// Whether neither side of the edge has a face.
    pub fn is_wire(&self, e: EdgeId) -> bool {
        let h = self.edge_half_edge(e);
        self.loop_of(h).is_none() && self.loop_of(self.twin(h)).is_none()
    }

    /// Whether exactly one side of the edge has a face.
    pub fn is_boundary(&self, e: EdgeId) -> bool {
        let h = self.edge_half_edge(e);
        self.loop_of(h).is_none() != self.loop_of(self.twin(h)).is_none()
    }

    /// The edge's shading.
    pub fn shading(&self, e: EdgeId) -> Shading {
        self.edges[e.slot()].shading
    }

    /// Sets the edge's shading.
    pub fn set_shading(&mut self, e: EdgeId, shading: Shading) {
        self.edges[e.slot()].shading = shading;
    }

    /// The edge joining two vertices, if any. O(valence of `a`).
    pub fn find_edge(&self, a: VertId, b: VertId) -> Option<EdgeId> {
        self.find_half_edge(a, b).map(|h| self.edge_of(h))
    }

    /// The half-edge running from `a` to `b`, if the edge exists. O(valence of `a`).
    pub fn find_half_edge(&self, a: VertId, b: VertId) -> Option<HalfEdgeId> {
        self.outgoing(a).find(|&h| self.dest(h) == b)
    }

    // ---- loops ----

    /// The face a loop bounds.
    pub fn loop_face(&self, l: LoopId) -> FaceId {
        self.loops[l.slot()].face
    }

    /// Whether a loop is a hole of its face, rather than its outer boundary.
    pub fn is_hole(&self, l: LoopId) -> bool {
        self.face_outer(self.loop_face(l)) != l
    }

    /// The half-edges around a loop, starting from its first.
    pub fn loop_half_edges(&self, l: LoopId) -> impl Iterator<Item = HalfEdgeId> + '_ {
        let first = self.loops[l.slot()].first;
        std::iter::successors(Some(first), move |&h| self.next(h).filter(|&n| n != first))
    }

    /// The vertices around a loop.
    pub fn loop_verts(&self, l: LoopId) -> impl Iterator<Item = VertId> + '_ {
        self.loop_half_edges(l).map(|h| self.origin(h))
    }

    /// The positions around a loop.
    pub fn loop_positions(&self, l: LoopId) -> Vec<DVec3> {
        self.loop_verts(l).map(|v| self.position(v)).collect()
    }

    /// The corners (vertex and UV) around a loop.
    pub fn loop_corners(&self, l: LoopId) -> Vec<Corner> {
        self.loop_half_edges(l)
            .map(|h| Corner {
                v: self.origin(h),
                uv: self.uv(h),
            })
            .collect()
    }

    // ---- faces ----

    /// A face's outer loop.
    pub fn face_outer(&self, f: FaceId) -> LoopId {
        self.faces[f.slot()].outer
    }

    /// A face's hole loops, in order.
    pub fn face_holes(&self, f: FaceId) -> impl Iterator<Item = LoopId> + '_ {
        std::iter::successors(self.loops[self.face_outer(f).slot()].next_hole, |l| {
            self.loops[l.slot()].next_hole
        })
    }

    /// A face's loops: the outer loop, then its holes.
    pub fn face_loops(&self, f: FaceId) -> impl Iterator<Item = LoopId> + '_ {
        std::iter::once(self.face_outer(f)).chain(self.face_holes(f))
    }

    /// Every half-edge of a face, over all its loops.
    pub fn face_half_edges(&self, f: FaceId) -> impl Iterator<Item = HalfEdgeId> + '_ {
        self.face_loops(f).flat_map(|l| self.loop_half_edges(l))
    }

    /// Every vertex position of a face, over all its loops.
    pub fn face_positions(&self, f: FaceId) -> Vec<DVec3> {
        self.face_half_edges(f)
            .map(|h| self.position(self.origin(h)))
            .collect()
    }

    /// The corners of each loop of a face: the outer loop, then its holes.
    pub fn face_corners(&self, f: FaceId) -> Vec<Vec<Corner>> {
        self.face_loops(f).map(|l| self.loop_corners(l)).collect()
    }

    /// A face's material.
    pub fn material(&self, f: FaceId) -> MaterialId {
        self.faces[f.slot()].material
    }

    /// Sets a face's material.
    pub fn set_material(&mut self, f: FaceId, material: MaterialId) {
        self.faces[f.slot()].material = material;
    }

    /// Unit normal of a face, from its outer loop (Newell's method). Zero for a degenerate face.
    pub fn face_normal(&self, f: FaceId) -> DVec3 {
        newell_normal(&self.loop_positions(self.face_outer(f))).normalize_or_zero()
    }
}
