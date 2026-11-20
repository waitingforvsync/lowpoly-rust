//! Primitive operators. Each checks its preconditions before changing anything, so an error
//! leaves the mesh unchanged.

use glam::{DVec2, DVec3};

use super::editable_mesh::{EdgeRecord, FaceRecord, HalfEdgeRecord, LoopRecord, VertRecord};
use super::{
    Corner, EdgeId, EditableMesh, FaceId, HalfEdgeId, LoopId, MaterialId, MeshError, Shading,
    VertId,
};

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
        let count = self.valence(v);
        if count > 0 {
            return Err(MeshError::VertexHasEdges { vert: v, count });
        }
        self.verts[v.slot()].alive = false;
        self.live.verts -= 1;
        Ok(())
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
            return Err(MeshError::EdgeExists(a, b));
        }
        let h = self.create_edge(a, b);
        Ok(self.edge_of(h))
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
            return Err(MeshError::EdgeBordersFace(a, b));
        }
        let h = self.edge_half_edge(e);
        let t = self.twin(h);
        self.unlink_out(h);
        self.unlink_out(t);
        self.edges[e.slot()].alive = false;
        self.live.edges -= 1;
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
        self.insert_face(None, loops, material)
    }

    /// Removes a face. Its edges remain, as boundary or wire edges, unless `cleanup` asks
    /// otherwise.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`].
    pub fn remove_face(&mut self, f: FaceId, cleanup: RemoveFace) -> Result<(), MeshError> {
        self.check_face(f)?;
        let half_edges: Vec<HalfEdgeId> = self.face_half_edges(f).collect();
        for l in self.face_loops(f).collect::<Vec<_>>() {
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
            for &h in &half_edges {
                let e = self.edge_of(h);
                if self.contains_edge(e) && self.is_wire(e) {
                    self.remove_edge(e)?;
                }
            }
        }
        if cleanup.isolated_verts {
            for &h in &half_edges {
                let v = self.origin(h);
                if self.contains_vert(v) && self.verts[v.slot()].first_out.is_none() {
                    self.remove_vertex(v)?;
                }
            }
        }
        Ok(())
    }

    /// Runs `edit` on a copy of the mesh and keeps the result only if it succeeds, so a failure
    /// part-way through leaves the mesh unchanged. The copy shares storage with the original, so
    /// this costs only the chunks `edit` writes.
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

    // ---- internals ----

    pub(super) fn check_vert(&self, v: VertId) -> Result<(), MeshError> {
        if self.contains_vert(v) {
            Ok(())
        } else {
            Err(MeshError::NoSuchVertex(v))
        }
    }

    pub(super) fn check_face(&self, f: FaceId) -> Result<(), MeshError> {
        if self.contains_face(f) {
            Ok(())
        } else {
            Err(MeshError::NoSuchFace(f))
        }
    }

    /// Adds a face, either new or reusing the slot of the dead face `id`, so a rebuilt face keeps
    /// its identity.
    pub(super) fn insert_face(
        &mut self,
        id: Option<FaceId>,
        loops: &[impl AsRef<[Corner]>],
        material: MaterialId,
    ) -> Result<FaceId, MeshError> {
        debug_assert!(
            id.is_none_or(|f| !self.contains_face(f)),
            "reused face slot must be dead"
        );
        if loops.is_empty() {
            return Err(MeshError::NoLoops);
        }
        let mut seen = Vec::new();
        for corners in loops {
            let corners = corners.as_ref();
            if corners.len() < 3 {
                return Err(MeshError::TooFewCorners(corners.len()));
            }
            for c in corners {
                self.check_vert(c.v)?;
                if seen.contains(&c.v) {
                    return Err(MeshError::RepeatedVertex(c.v));
                }
                seen.push(c.v);
            }
        }
        for corners in loops {
            for (a, b) in cyclic_pairs(corners.as_ref()) {
                if let Some(h) = self.find_half_edge(a.v, b.v)
                    && let Some(face) = self.face_of(h)
                {
                    return Err(MeshError::HalfEdgeInUse {
                        from: a.v,
                        to: b.v,
                        face,
                    });
                }
            }
        }

        // All checks passed: mutate.
        let face_record = FaceRecord {
            outer: LoopId::new(0),
            material,
            alive: true,
        };
        let f = match id {
            Some(f) => {
                self.faces[f.slot()] = face_record;
                f
            }
            None => FaceId::from_slot(self.faces.push(face_record)),
        };
        self.live.faces += 1;

        let mut previous_loop: Option<LoopId> = None;
        for corners in loops {
            let corners = corners.as_ref();
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
            for (i, (&h, corner)) in half_edges.iter().zip(corners).enumerate() {
                let record = &mut self.half_edges[h.slot()];
                record.loop_id = Some(l);
                record.next = Some(half_edges[(i + 1) % n]);
                record.prev = Some(half_edges[(i + n - 1) % n]);
                self.uvs[h.slot()] = corner.uv;
            }
            match previous_loop {
                None => self.faces[f.slot()].outer = l,
                Some(p) => self.loops[p.slot()].next_hole = Some(l),
            }
            previous_loop = Some(l);
        }
        Ok(f)
    }

    /// Creates an edge from `a` to `b` and returns its primary half-edge (`a` to `b`).
    fn create_edge(&mut self, a: VertId, b: VertId) -> HalfEdgeId {
        let slot = self.edges.push(EdgeRecord {
            alive: true,
            shading: Shading::Hard,
        });
        self.live.edges += 1;
        let mut half = |origin: VertId| {
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
        };
        let h = half(a);
        half(b);
        debug_assert_eq!(
            h.slot(),
            slot * 2,
            "half-edges are allocated in pairs with their edge"
        );
        h
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
            .expect("half-edge is in its origin's out list");
        self.half_edges[before.slot()].next_out = after;
    }
}

/// Consecutive pairs around a closed loop: (c0, c1), (c1, c2), ..., (cn-1, c0).
fn cyclic_pairs(corners: &[Corner]) -> impl Iterator<Item = (Corner, Corner)> + '_ {
    corners
        .iter()
        .enumerate()
        .map(|(i, &a)| (a, corners[(i + 1) % corners.len()]))
}
