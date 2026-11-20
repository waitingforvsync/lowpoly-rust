//! Topological operators built on the primitives. Each rebuilds the faces it changes through
//! `remove_face`/`insert_face` (faces are small, so this costs little and keeps all link updates
//! in one validated place) inside a [`EditableMesh::transaction`], so an error leaves the mesh unchanged.
//! Rebuilt faces keep their ids.

use glam::{DVec2, DVec3};

use super::{Corner, EdgeId, EditableMesh, FaceId, LoopId, MeshError, RemoveFace, VertId};
use crate::math::{newell_normal, plane_projector, point_in_polygon_2d};
use crate::planarity::is_planar;
use crate::uv::face_uv_function;

/// A face loop being rebuilt; `None` marks a vertex that doesn't exist yet.
type PlannedLoop = Vec<(Option<VertId>, DVec2)>;

/// Whether [`EditableMesh::add_hole`] also creates a face filling the hole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HoleFill {
    /// Leave the hole open.
    #[default]
    Open,
    /// Fill the hole with a new face, as when drawing a shape on a face in SketchUp.
    Filled,
}

/// The result of [`EditableMesh::add_hole`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddedHole {
    /// The new hole loop.
    pub hole: LoopId,
    /// The face filling the hole, with [`HoleFill::Filled`].
    pub fill: Option<FaceId>,
}

impl EditableMesh {
    /// Inserts a new vertex on an edge at parameter `t` (0 at the origin of the edge's primary
    /// half-edge, 1 at its destination). Both adjacent faces gain the corner, with UVs
    /// interpolated per face, and both halves keep the edge's shading.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchEdge`].
    pub fn split_edge(&mut self, e: EdgeId, t: f64) -> Result<VertId, MeshError> {
        if !self.contains_edge(e) {
            return Err(MeshError::NoSuchEdge(e));
        }
        let [a, b] = self.edge_verts(e);
        let pos = self.position(a).lerp(self.position(b), t);
        let faces: Vec<FaceId> = self.edge_faces(e).collect();
        let shading = self.shading(e);

        // New corner lists for each adjacent face; `None` marks the new vertex.
        let plans: Vec<(FaceId, Vec<PlannedLoop>)> = faces
            .iter()
            .map(|&f| {
                let loops = self
                    .face_corners(f)
                    .iter()
                    .map(|corners| {
                        let mut out = Vec::with_capacity(corners.len() + 1);
                        for (i, c) in corners.iter().enumerate() {
                            out.push((Some(c.v), c.uv));
                            let n = corners[(i + 1) % corners.len()];
                            if (c.v == a && n.v == b) || (c.v == b && n.v == a) {
                                let local = if c.v == a { t } else { 1.0 - t };
                                out.push((None, c.uv.lerp(n.uv, local)));
                            }
                        }
                        out
                    })
                    .collect();
                (f, loops)
            })
            .collect();

        self.transaction(|m| {
            for &f in &faces {
                m.remove_face(f, RemoveFace::default())?;
            }
            m.remove_edge(e)?;
            let v = m.add_vertex(pos);
            if faces.is_empty() {
                m.add_edge(a, v)?;
                m.add_edge(v, b)?;
            }
            for (f, loops) in &plans {
                let loops: Vec<Vec<Corner>> = loops
                    .iter()
                    .map(|l| {
                        l.iter()
                            .map(|&(cv, uv)| Corner {
                                v: cv.unwrap_or(v),
                                uv,
                            })
                            .collect()
                    })
                    .collect();
                let material = m.material(*f);
                m.insert_face(Some(*f), &loops, material)?;
            }
            for (x, y) in [(a, v), (v, b)] {
                if let Some(half) = m.find_edge(x, y) {
                    m.set_shading(half, shading);
                }
            }
            Ok(v)
        })
    }

    /// Splits a face by joining two vertices of its outer loop with a new edge, or with a chain
    /// of new edges through `path` (points inside the face, in its plane, from `va` to `vb`).
    ///
    /// The original face keeps its id and the side running from `va` to `vb`; the other side
    /// becomes a new face. Holes go to whichever side contains them. Path corners get UVs that
    /// continue the face's texture. Without a path, the vertices must not be adjacent.
    ///
    /// Returns the kept face and the new face.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`], [`MeshError::NotOnOuterLoop`], [`MeshError::SameVertex`],
    /// [`MeshError::AdjacentVertices`], [`MeshError::EdgeExists`], [`MeshError::PathNotPlanar`] or
    /// [`MeshError::CrossesHole`].
    pub fn split_face(
        &mut self,
        f: FaceId,
        va: VertId,
        vb: VertId,
        path: &[DVec3],
    ) -> Result<(FaceId, FaceId), MeshError> {
        self.check_face(f)?;
        let loops = self.face_corners(f);
        let outer = &loops[0];
        let n = outer.len();
        let (Some(i), Some(j)) = (
            outer.iter().position(|c| c.v == va),
            outer.iter().position(|c| c.v == vb),
        ) else {
            return Err(MeshError::NotOnOuterLoop);
        };
        if i == j {
            return Err(MeshError::SameVertex);
        }
        if path.is_empty() {
            if (i + 1) % n == j || (j + 1) % n == i {
                return Err(MeshError::AdjacentVertices);
            }
            if self.find_edge(va, vb).is_some() {
                return Err(MeshError::EdgeExists(va, vb));
            }
        } else {
            let mut points = self.face_positions(f);
            points.extend_from_slice(path);
            if !is_planar(&points) {
                return Err(MeshError::PathNotPlanar);
            }
        }

        let side = |from: usize, to: usize| -> Vec<Corner> {
            let mut out = Vec::new();
            let mut k = from;
            loop {
                out.push(outer[k]);
                if k == to {
                    return out;
                }
                k = (k + 1) % n;
            }
        };
        let side_a = side(i, j);
        let side_b = side(j, i);

        let outer_positions: Vec<DVec3> = outer.iter().map(|c| self.position(c.v)).collect();
        let path_uvs: Vec<DVec2> = if path.is_empty() {
            Vec::new()
        } else {
            let outer_uvs: Vec<DVec2> = outer.iter().map(|c| c.uv).collect();
            let uv_at = face_uv_function(&outer_positions, &outer_uvs);
            path.iter().map(|&p| uv_at(p)).collect()
        };

        // Distribute holes by containment in side A (which runs back along the path).
        let project = plane_projector(&outer_positions);
        let poly_a: Vec<DVec2> = side_a
            .iter()
            .map(|c| project(self.position(c.v)))
            .chain(path.iter().rev().map(|&p| project(p)))
            .collect();
        let mut holes_a = Vec::new();
        let mut holes_b = Vec::new();
        for hole in &loops[1..] {
            let inside: Vec<bool> = hole
                .iter()
                .map(|c| point_in_polygon_2d(project(self.position(c.v)), &poly_a))
                .collect();
            if inside.iter().all(|&x| x) {
                holes_a.push(hole.clone());
            } else if inside.iter().all(|&x| !x) {
                holes_b.push(hole.clone());
            } else {
                return Err(MeshError::CrossesHole);
            }
        }
        let material = self.material(f);

        self.transaction(|m| {
            m.remove_face(f, RemoveFace::default())?;
            let path_corners: Vec<Corner> = path
                .iter()
                .zip(&path_uvs)
                .map(|(&p, &uv)| Corner {
                    v: m.add_vertex(p),
                    uv,
                })
                .collect();
            let mut loop_a = side_a;
            loop_a.extend(path_corners.iter().rev());
            let mut loop_b = side_b;
            loop_b.extend(path_corners.iter());

            let loops_a: Vec<Vec<Corner>> = std::iter::once(loop_a).chain(holes_a).collect();
            let loops_b: Vec<Vec<Corner>> = std::iter::once(loop_b).chain(holes_b).collect();
            let fa = m.insert_face(Some(f), &loops_a, material)?;
            let fb = m.insert_face(None, &loops_b, material)?;
            Ok((fa, fb))
        })
    }

    /// Dissolves an edge between two different faces, merging them into one. The face on the
    /// side of the edge's primary half-edge keeps its id, material, and UVs at the edge's
    /// endpoints. Both sides must be outer loops, the faces must share only this edge, and they
    /// must be coplanar.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchEdge`], [`MeshError::NotInteriorEdge`],
    /// [`MeshError::SameFaceBothSides`], [`MeshError::NotOuterLoops`], [`MeshError::SharedEdges`],
    /// [`MeshError::FacesTouch`] or [`MeshError::NotCoplanar`].
    pub fn join_faces(&mut self, e: EdgeId) -> Result<FaceId, MeshError> {
        if !self.contains_edge(e) {
            return Err(MeshError::NoSuchEdge(e));
        }
        let h = self.edge_half_edge(e);
        let t = self.twin(h);
        let (Some(f1), Some(f2)) = (self.face_of(h), self.face_of(t)) else {
            return Err(MeshError::NotInteriorEdge);
        };
        if f1 == f2 {
            return Err(MeshError::SameFaceBothSides);
        }
        if self.loop_of(h) != Some(self.face_outer(f1))
            || self.loop_of(t) != Some(self.face_outer(f2))
        {
            return Err(MeshError::NotOuterLoops);
        }
        let shared = self
            .loop_half_edges(self.face_outer(f1))
            .filter(|&x| self.face_of(self.twin(x)) == Some(f2))
            .count();
        if shared != 1 {
            return Err(MeshError::SharedEdges(shared));
        }

        // f1 from h.next (corner at b) round to h (corner at a): [b, ..., a].
        // f2 from t.next (corner at a) round to t (corner at b): [a, ..., b], minus both ends.
        let (Some(after_h), Some(after_t)) = (self.next(h), self.next(t)) else {
            return Err(MeshError::NotInteriorEdge);
        };
        let first = self.corners_from(after_h);
        let second = self.corners_from(after_t);
        let mut merged = first;
        merged.extend_from_slice(&second[1..second.len() - 1]);

        let mut seen: Vec<VertId> = merged.iter().map(|c| c.v).collect();
        seen.sort_unstable();
        if seen.windows(2).any(|w| w[0] == w[1]) {
            return Err(MeshError::FacesTouch);
        }
        let mut points = self.face_positions(f1);
        points.extend(self.face_positions(f2));
        if !is_planar(&points) {
            return Err(MeshError::NotCoplanar);
        }
        let holes: Vec<Vec<Corner>> = self
            .face_holes(f1)
            .chain(self.face_holes(f2))
            .map(|l| self.loop_corners(l))
            .collect();
        let material = self.material(f1);

        self.transaction(|m| {
            m.remove_face(f1, RemoveFace::default())?;
            m.remove_face(f2, RemoveFace::default())?;
            m.remove_edge(e)?;
            let loops: Vec<Vec<Corner>> = std::iter::once(merged).chain(holes).collect();
            m.insert_face(Some(f1), &loops, material)
        })
    }

    /// Cuts a hole into a face, bounded by vertices not already in it. The corners may wind
    /// either way; they are reoriented to wind opposite to the face. With [`HoleFill::Filled`],
    /// a new face fills the hole.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`], [`MeshError::NoSuchVertex`], [`MeshError::HoleVertexInFace`],
    /// [`MeshError::NotCoplanar`], or any error from adding the face.
    pub fn add_hole(
        &mut self,
        f: FaceId,
        corners: &[Corner],
        fill: HoleFill,
    ) -> Result<AddedHole, MeshError> {
        self.check_face(f)?;
        for c in corners {
            self.check_vert(c.v)?;
        }
        let loops = self.face_corners(f);
        if corners
            .iter()
            .any(|c| loops.iter().flatten().any(|x| x.v == c.v))
        {
            return Err(MeshError::HoleVertexInFace);
        }
        let mut hole = corners.to_vec();
        let hole_positions: Vec<DVec3> = hole.iter().map(|c| self.position(c.v)).collect();
        let mut points = self.face_positions(f);
        points.extend_from_slice(&hole_positions);
        if !is_planar(&points) {
            return Err(MeshError::NotCoplanar);
        }
        let face_normal = newell_normal(&self.loop_positions(self.face_outer(f)));
        if face_normal.dot(newell_normal(&hole_positions)) > 0.0 {
            hole.reverse();
        }
        let material = self.material(f);

        self.transaction(|m| {
            m.remove_face(f, RemoveFace::default())?;
            let mut all = loops;
            all.push(hole.clone());
            m.insert_face(Some(f), &all, material)?;
            let Some(hole_loop) = m.face_holes(f).last() else {
                return Err(MeshError::Invalid(vec![format!(
                    "face {f} lost its new hole"
                )]));
            };
            let fill = match fill {
                HoleFill::Open => None,
                HoleFill::Filled => {
                    hole.reverse();
                    Some(m.insert_face(None, &[hole], material)?)
                }
            };
            Ok(AddedHole {
                hole: hole_loop,
                fill,
            })
        })
    }

    /// Creates a face filling a hole of `f`. It winds opposite to the hole and copies its UVs.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`], [`MeshError::NotAHole`], or any error from adding the face
    /// (for example if the hole is already filled).
    pub fn fill_hole(&mut self, f: FaceId, hole: LoopId) -> Result<FaceId, MeshError> {
        self.check_hole(f, hole)?;
        let mut corners = self.loop_corners(hole);
        corners.reverse();
        let material = self.material(f);
        self.insert_face(None, &[corners], material)
    }

    /// Removes an open hole (one no face fills) from a face, so the face covers that area. Hole
    /// edges left as wire edges, and vertices left isolated, are deleted.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`], [`MeshError::NotAHole`] or [`MeshError::HoleFilled`].
    pub fn remove_hole(&mut self, f: FaceId, hole: LoopId) -> Result<(), MeshError> {
        self.check_hole(f, hole)?;
        if self
            .loop_half_edges(hole)
            .any(|h| self.loop_of(self.twin(h)).is_some())
        {
            return Err(MeshError::HoleFilled);
        }
        let hole_verts: Vec<VertId> = self.loop_verts(hole).collect();
        let loops: Vec<Vec<Corner>> = self
            .face_loops(f)
            .filter(|&l| l != hole)
            .map(|l| self.loop_corners(l))
            .collect();
        let material = self.material(f);

        self.transaction(|m| {
            m.remove_face(f, RemoveFace::default())?;
            m.insert_face(Some(f), &loops, material)?;
            for (i, &a) in hole_verts.iter().enumerate() {
                let b = hole_verts[(i + 1) % hole_verts.len()];
                if let Some(e) = m.find_edge(a, b)
                    && m.is_wire(e)
                {
                    m.remove_edge(e)?;
                }
            }
            for &v in &hole_verts {
                if m.valence(v) == 0 {
                    m.remove_vertex(v)?;
                }
            }
            Ok(())
        })
    }

    /// The corners of a loop, starting from the half-edge `start`.
    fn corners_from(&self, start: super::HalfEdgeId) -> Vec<Corner> {
        std::iter::successors(Some(start), |&x| self.next(x).filter(|&n| n != start))
            .map(|x| Corner {
                v: self.origin(x),
                uv: self.uv(x),
            })
            .collect()
    }

    fn check_hole(&self, f: FaceId, hole: LoopId) -> Result<(), MeshError> {
        self.check_face(f)?;
        if self.contains_loop(hole) && self.loop_face(hole) == f && self.is_hole(hole) {
            Ok(())
        } else {
            Err(MeshError::NotAHole(hole))
        }
    }
}
