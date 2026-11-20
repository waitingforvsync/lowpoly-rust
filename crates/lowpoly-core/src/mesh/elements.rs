//! Adding and removing vertices, edges and faces.
//!
//! Each public operator checks its preconditions, then calls an infallible internal helper
//! (`kill_*`, `link_face`, `create_edge`), so an error always leaves the mesh unchanged. The
//! topological operators in `euler` use the same helpers after their own checks.

use glam::{DVec2, DVec3};

use super::editable_mesh::{EdgeRecord, FaceRecord, HalfEdgeRecord, LoopRecord, VertRecord};
use super::{
    Corner, EdgeId, EditableMesh, FaceId, HalfEdgeId, LoopId, MaterialId, MeshError, Shading,
    VertId,
};
use crate::math::cyclic_pairs;

/// What [`EditableMesh::remove_face`] cleans up besides the face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemoveFace {
    /// Also remove edges left with no face on either side.
    pub wire_edges: bool,
    /// Also remove vertices left with no edges.
    pub isolated_verts: bool,
}

impl EditableMesh {
    /// Adds a vertex with no edges.
    pub fn add_vertex(&mut self, p: DVec3) -> VertId {
        self.positions.push(p);
        let slot = self.verts.push(VertRecord {
            first_out: None,
            alive: true,
        });
        self.live.verts += 1;
        VertId::from_slot(slot)
    }

    /// Removes a vertex that has no edges.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchVertex`] or [`MeshError::VertexHasEdges`].
    pub fn remove_vertex(&mut self, v: VertId) -> Result<(), MeshError> {
        self.check_vert(v)?;
        match self.valence(v) {
            0 => {
                self.kill_vertex(v);
                Ok(())
            }
            count => Err(MeshError::VertexHasEdges { vert: v, count }),
        }
    }

    /// Adds a wire edge (one with no faces) between two vertices.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchVertex`], [`MeshError::SelfEdge`] or [`MeshError::EdgeExists`].
    pub fn add_edge(&mut self, a: VertId, b: VertId) -> Result<EdgeId, MeshError> {
        self.check_vert(a)?;
        self.check_vert(b)?;
        if a == b {
            return Err(MeshError::SelfEdge(a));
        }
        if self.find_edge(a, b).is_some() {
            return Err(MeshError::EdgeExists { a, b });
        }
        Ok(self.create_edge(a, b).edge())
    }

    /// Removes an edge with no face on either side.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchEdge`] or [`MeshError::EdgeBordersFace`].
    pub fn remove_edge(&mut self, e: EdgeId) -> Result<(), MeshError> {
        if !self.contains_edge(e) {
            return Err(MeshError::NoSuchEdge(e));
        }
        if !self.is_wire(e) {
            let [a, b] = self.edge_verts(e);
            return Err(MeshError::EdgeBordersFace { a, b });
        }
        self.kill_edge(e);
        Ok(())
    }

    /// Adds a face. `loops[0]` is the outer boundary, counter-clockwise about the intended normal;
    /// any further loops are holes, clockwise. Missing edges are created. Existing edges must have
    /// the matching half-edge free, or the face would be non-manifold or wind opposite to its
    /// neighbour.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoLoops`], [`MeshError::TooFewCorners`], [`MeshError::NoSuchVertex`],
    /// [`MeshError::RepeatedVertex`] or [`MeshError::HalfEdgeInUse`].
    pub fn add_face(
        &mut self,
        loops: &[impl AsRef<[Corner]>],
        material: MaterialId,
    ) -> Result<FaceId, MeshError> {
        self.check_new_face(loops, &[])?;
        Ok(self.link_face(None, loops, material))
    }

    /// Removes a face. Its edges remain, as boundary or wire edges, unless `cleanup` asks
    /// otherwise.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`].
    pub fn remove_face(&mut self, f: FaceId, cleanup: RemoveFace) -> Result<(), MeshError> {
        self.check_face(f)?;
        self.kill_face(f, cleanup);
        Ok(())
    }

    /// Runs `edit` on a copy of the mesh and keeps the result only if it succeeds, so a failure
    /// part-way through a multi-step edit leaves the mesh unchanged.
    ///
    /// The copy is O(1). Writes through it then pay the usual copy-on-write cost: the first write
    /// to each attribute copies that attribute's chunk table (one pointer per chunk), and each
    /// chunk written is copied once. The operators on this type are already atomic, so this is
    /// for composing several of them.
    ///
    /// # Errors
    ///
    /// Whatever `edit` returns.
    pub fn transaction<T>(
        &mut self,
        edit: impl FnOnce(&mut Self) -> Result<T, MeshError>,
    ) -> Result<T, MeshError> {
        let mut scratch = self.clone();
        let result = edit(&mut scratch)?;
        *self = scratch;
        Ok(result)
    }

    // ---- checks ----

    pub(super) fn check_vert(&self, v: VertId) -> Result<(), MeshError> {
        self.contains_vert(v)
            .then_some(())
            .ok_or(MeshError::NoSuchVertex(v))
    }

    pub(super) fn check_face(&self, f: FaceId) -> Result<(), MeshError> {
        self.contains_face(f)
            .then_some(())
            .ok_or(MeshError::NoSuchFace(f))
    }

    /// Checks that `loops` would make a valid new face, treating the half-edges of the faces in
    /// `freed` as free (they are about to be removed and rebuilt).
    pub(super) fn check_new_face(
        &self,
        loops: &[impl AsRef<[Corner]>],
        freed: &[FaceId],
    ) -> Result<(), MeshError> {
        if loops.is_empty() {
            return Err(MeshError::NoLoops);
        }
        let short = loops.iter().map(AsRef::as_ref).find(|c| c.len() < 3);
        if let Some(corners) = short {
            return Err(MeshError::TooFewCorners(corners.len()));
        }
        let corners = || loops.iter().flat_map(AsRef::as_ref);
        corners().try_for_each(|c| self.check_vert(c.v))?;
        if let Some(v) = repeated_vertex(corners().map(|c| c.v)) {
            return Err(MeshError::RepeatedVertex(v));
        }

        loops
            .iter()
            .flat_map(|l| cyclic_pairs(l.as_ref()))
            .find_map(|(a, b)| {
                let h = self.find_half_edge(a.v, b.v)?;
                let face = self.face_of(h).filter(|f| !freed.contains(f))?;
                Some(MeshError::HalfEdgeInUse {
                    from: a.v,
                    to: b.v,
                    face,
                })
            })
            .map_or(Ok(()), Err)
    }

    // ---- infallible mutation, after checks ----

    /// Links a checked face, either new or reusing the slot of the dead face `slot`, so a rebuilt
    /// face keeps its identity. Missing edges are created.
    pub(super) fn link_face(
        &mut self,
        slot: Option<FaceId>,
        loops: &[impl AsRef<[Corner]>],
        material: MaterialId,
    ) -> FaceId {
        debug_assert!(
            slot.is_none_or(|f| !self.contains_face(f)),
            "a reused face slot must be dead"
        );
        // Storage is append-only, so a new face's handle is the next slot.
        let f = slot.unwrap_or_else(|| FaceId::from_slot(self.faces.len()));
        let (outer, holes) = loops
            .split_first()
            .expect("checked faces have an outer loop");
        let outer = self.link_loop(f, outer.as_ref());
        // Chain each hole from the loop before it.
        holes.iter().fold(outer, |previous, corners| {
            let l = self.link_loop(f, corners.as_ref());
            self.loops[previous.slot()].next_hole = Some(l);
            l
        });
        let record = FaceRecord {
            outer,
            material,
            alive: true,
        };
        match slot {
            Some(f) => self.faces[f.slot()] = record,
            None => {
                self.faces.push(record);
            }
        }
        self.live.faces += 1;
        f
    }

    /// Links one loop of face `f`, creating missing edges.
    fn link_loop(&mut self, f: FaceId, corners: &[Corner]) -> LoopId {
        let half_edges: Vec<HalfEdgeId> = cyclic_pairs(corners)
            .map(|(a, b)| {
                self.find_half_edge(a.v, b.v)
                    .unwrap_or_else(|| self.create_edge(a.v, b.v))
            })
            .collect();
        let l = LoopId::from_slot(self.loops.push(LoopRecord {
            face: f,
            first: half_edges[0],
            next_hole: None,
            alive: true,
        }));
        let n = half_edges.len();
        let prevs = half_edges.iter().cycle().skip(n - 1);
        let nexts = half_edges.iter().cycle().skip(1);
        for (((&h, &prev), &next), corner) in half_edges.iter().zip(prevs).zip(nexts).zip(corners) {
            let record = &mut self.half_edges[h.slot()];
            record.loop_id = Some(l);
            record.next = Some(next);
            record.prev = Some(prev);
            self.uvs[h.slot()] = corner.uv;
        }
        l
    }

    /// Creates an edge from `a` to `b` and returns its primary half-edge (`a` to `b`).
    pub(super) fn create_edge(&mut self, a: VertId, b: VertId) -> HalfEdgeId {
        let slot = self.edges.push(EdgeRecord {
            alive: true,
            shading: Shading::Hard,
        });
        self.live.edges += 1;
        let h = self.push_half_edge(a);
        self.push_half_edge(b);
        debug_assert_eq!(
            h,
            EdgeId::from_slot(slot).half_edge(),
            "half-edges are allocated in pairs with their edge"
        );
        h
    }

    /// Appends a half-edge starting at `origin` and adds it to the front of its out list.
    fn push_half_edge(&mut self, origin: VertId) -> HalfEdgeId {
        let next_out = self.verts[origin.slot()].first_out;
        let h = HalfEdgeId::from_slot(self.half_edges.push(HalfEdgeRecord {
            origin,
            next: None,
            prev: None,
            loop_id: None,
            next_out,
        }));
        self.uvs.push(DVec2::ZERO);
        self.verts[origin.slot()].first_out = Some(h);
        h
    }

    /// Marks a vertex with no edges dead.
    pub(super) fn kill_vertex(&mut self, v: VertId) {
        debug_assert_eq!(self.valence(v), 0, "vertex {v} still has edges");
        self.verts[v.slot()].alive = false;
        self.live.verts -= 1;
    }

    /// Unlinks a wire edge from its vertices and marks it dead.
    pub(super) fn kill_edge(&mut self, e: EdgeId) {
        debug_assert!(self.is_wire(e), "edge {e} still borders a face");
        let h = e.half_edge();
        self.unlink_out(h);
        self.unlink_out(h.twin());
        self.edges[e.slot()].alive = false;
        self.live.edges -= 1;
    }

    /// Removes a live face, freeing its half-edges, then applies `cleanup`.
    pub(super) fn kill_face(&mut self, f: FaceId, cleanup: RemoveFace) {
        let half_edges: Vec<HalfEdgeId> = self.face_half_edges(f).collect();
        let loops: Vec<LoopId> = self.face_loops(f).collect();
        let verts: Vec<VertId> = half_edges.iter().map(|&h| self.origin(h)).collect();
        for l in loops {
            self.loops[l.slot()].alive = false;
        }
        for &h in &half_edges {
            let record = &mut self.half_edges[h.slot()];
            record.loop_id = None;
            record.next = None;
            record.prev = None;
            self.uvs[h.slot()] = DVec2::ZERO;
        }
        self.faces[f.slot()].alive = false;
        self.live.faces -= 1;

        if cleanup.wire_edges {
            for e in half_edges.iter().map(|h| h.edge()) {
                if self.contains_edge(e) && self.is_wire(e) {
                    self.kill_edge(e);
                }
            }
        }
        if cleanup.isolated_verts {
            for v in verts {
                if self.contains_vert(v) && self.valence(v) == 0 {
                    self.kill_vertex(v);
                }
            }
        }
    }

    /// Removes a half-edge from its origin's out list.
    fn unlink_out(&mut self, h: HalfEdgeId) {
        let v = self.origin(h);
        let after = self.half_edges[h.slot()].next_out;
        if self.verts[v.slot()].first_out == Some(h) {
            self.verts[v.slot()].first_out = after;
            return;
        }
        let before = self
            .outgoing(v)
            .find(|&x| self.half_edges[x.slot()].next_out == Some(h))
            .expect("a live half-edge is in its origin's out list");
        self.half_edges[before.slot()].next_out = after;
    }
}

/// The first vertex that appears more than once, if any. Faces are usually small, where a
/// pairwise scan beats allocating and sorting; large ones sort.
fn repeated_vertex(verts: impl Iterator<Item = VertId> + Clone) -> Option<VertId> {
    const SCAN_LIMIT: usize = 32;
    if verts.clone().nth(SCAN_LIMIT).is_none() {
        verts
            .clone()
            .enumerate()
            .find(|&(i, v)| verts.clone().skip(i + 1).any(|w| w == v))
            .map(|(_, v)| v)
    } else {
        let mut sorted: Vec<VertId> = verts.collect();
        sorted.sort_unstable();
        sorted.windows(2).find(|w| w[0] == w[1]).map(|w| w[0])
    }
}
