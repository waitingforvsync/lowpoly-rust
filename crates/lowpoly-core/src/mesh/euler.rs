//! Topological operators built on the element helpers.
//!
//! Each operator rebuilds the faces it changes: faces are small, so this costs little and keeps
//! all link updates in `link_face`. Each first checks everything that could fail, including the
//! rebuilt faces themselves (`check_new_face`), then mutates with infallible helpers, so an error
//! leaves the mesh unchanged without a transaction. Rebuilt faces keep their ids; their loops
//! get new ids.
//!
//! Storage is append-only, so the handles of elements an operator is about to create are known
//! in advance, and the final corner lists are built before anything changes.

use std::iter;

use glam::{DVec2, DVec3};

use super::{
    Corner, EdgeId, EditableMesh, FaceId, LoopId, MaterialId, MeshError, RemoveFace, VertId,
};
use crate::math::{PlaneProjector, cyclic_pairs, newell_normal, point_in_polygon_2d};
use crate::planarity::is_planar;
use crate::uv::face_uv_mapping;

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
    /// The new hole loop. Like every loop id, it changes when a later operator rebuilds the face.
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
    /// [`MeshError::NoSuchEdge`] or [`MeshError::NonFinite`].
    pub fn split_edge(&mut self, e: EdgeId, t: f64) -> Result<VertId, MeshError> {
        if !self.contains_edge(e) {
            return Err(MeshError::NoSuchEdge(e));
        }
        if !t.is_finite() {
            return Err(MeshError::NonFinite);
        }
        let [a, b] = self.edge_verts(e);
        let pos = self.position(a).lerp(self.position(b), t);
        let shading = self.shading(e);
        let v = VertId::from_slot(self.verts.len());
        let plans: Vec<(FaceId, MaterialId, Vec<Vec<Corner>>)> = self
            .edge_faces(e)
            .map(|f| {
                let loops = self
                    .face_corners(f)
                    .iter()
                    .map(|corners| with_corner_on_edge(corners, [a, b], t, v))
                    .collect();
                (f, self.material(f), loops)
            })
            .collect();

        for &(f, _, _) in &plans {
            self.kill_face(f, RemoveFace::default());
        }
        self.kill_edge(e);
        let added = self.add_vertex(pos);
        debug_assert_eq!(added, v, "new vertices are appended");
        if plans.is_empty() {
            self.create_edge(a, v);
            self.create_edge(v, b);
        }
        for (f, material, loops) in &plans {
            self.link_face(Some(*f), loops, *material);
        }
        let halves: Vec<EdgeId> = [(a, v), (v, b)]
            .into_iter()
            .filter_map(|(x, y)| self.find_edge(x, y))
            .collect();
        for half in halves {
            self.set_shading(half, shading);
        }
        Ok(v)
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
    /// [`MeshError::NoSuchFace`], [`MeshError::NonFinite`], [`MeshError::NotOnOuterLoop`],
    /// [`MeshError::SameVertex`], [`MeshError::AdjacentVertices`], [`MeshError::EdgeExists`],
    /// [`MeshError::PathNotPlanar`] or [`MeshError::CrossesHole`].
    pub fn split_face(
        &mut self,
        f: FaceId,
        va: VertId,
        vb: VertId,
        path: &[DVec3],
    ) -> Result<(FaceId, FaceId), MeshError> {
        self.check_face(f)?;
        if path.iter().any(|p| !p.is_finite()) {
            return Err(MeshError::NonFinite);
        }
        let mut loops = self.face_corners(f);
        let holes = loops.split_off(1);
        let outer = &loops[0];
        let n = outer.len();
        let position_of = |v: VertId| outer.iter().position(|c| c.v == v);
        let (Some(i), Some(j)) = (position_of(va), position_of(vb)) else {
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
                return Err(MeshError::EdgeExists { a: va, b: vb });
            }
        } else if !is_planar(self.face_positions(f).chain(path.iter().copied())) {
            return Err(MeshError::PathNotPlanar);
        }

        // The corners from `from` round to `to`, inclusive.
        let side = |from: usize, to: usize| -> Vec<Corner> {
            outer
                .iter()
                .cycle()
                .skip(from)
                .take((to + n - from) % n + 1)
                .copied()
                .collect()
        };
        let side_a = side(i, j);
        let side_b = side(j, i);

        let outer_positions: Vec<DVec3> = outer.iter().map(|c| self.position(c.v)).collect();
        let path_uvs: Vec<DVec2> = if path.is_empty() {
            Vec::new()
        } else {
            let outer_uvs: Vec<DVec2> = outer.iter().map(|c| c.uv).collect();
            let mapping = face_uv_mapping(&outer_positions, &outer_uvs);
            path.iter().map(|&p| mapping.uv(p)).collect()
        };

        // Distribute holes by containment in side A, which runs back along the path.
        let plane = PlaneProjector::new(outer_positions.iter().copied());
        let poly_a: Vec<DVec2> = side_a
            .iter()
            .map(|c| plane.project(self.position(c.v)))
            .chain(path.iter().rev().map(|&p| plane.project(p)))
            .collect();
        let mut holes_a = Vec::new();
        let mut holes_b = Vec::new();
        for hole in holes {
            let inside = hole
                .iter()
                .filter(|c| point_in_polygon_2d(plane.project(self.position(c.v)), &poly_a))
                .count();
            match inside {
                0 => holes_b.push(hole),
                k if k == hole.len() => holes_a.push(hole),
                _ => return Err(MeshError::CrossesHole),
            }
        }

        let first_new = self.verts.len();
        let path_corners: Vec<Corner> = path_uvs
            .iter()
            .enumerate()
            .map(|(k, &uv)| Corner {
                v: VertId::from_slot(first_new + k),
                uv,
            })
            .collect();
        let loop_a: Vec<Corner> = side_a
            .into_iter()
            .chain(path_corners.iter().rev().copied())
            .collect();
        let loop_b: Vec<Corner> = side_b
            .into_iter()
            .chain(path_corners.iter().copied())
            .collect();
        let loops_a: Vec<Vec<Corner>> = iter::once(loop_a).chain(holes_a).collect();
        let loops_b: Vec<Vec<Corner>> = iter::once(loop_b).chain(holes_b).collect();
        let material = self.material(f);

        self.kill_face(f, RemoveFace::default());
        for &p in path {
            self.add_vertex(p);
        }
        self.link_face(Some(f), &loops_a, material);
        let fb = self.link_face(None, &loops_b, material);
        Ok((f, fb))
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
    /// [`MeshError::FacesTouch`], [`MeshError::NotCoplanar`], or [`MeshError::RepeatedVertex`]
    /// when a hole of one face touches the other face.
    pub fn join_faces(&mut self, e: EdgeId) -> Result<FaceId, MeshError> {
        if !self.contains_edge(e) {
            return Err(MeshError::NoSuchEdge(e));
        }
        let h = e.half_edge();
        let t = h.twin();
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
            .filter(|&x| self.face_of(x.twin()) == Some(f2))
            .count();
        if shared != 1 {
            return Err(MeshError::SharedEdges(shared));
        }

        // f1 from h.next (corner at b) round to h (corner at a): [b, ..., a].
        // f2 from t.next (corner at a) round to t (corner at b): [a, ..., b], minus both ends.
        let (Some(after_h), Some(after_t)) = (self.next(h), self.next(t)) else {
            return Err(MeshError::NotInteriorEdge);
        };
        let second: Vec<Corner> = self
            .half_edges_from(after_t)
            .map(|x| self.corner(x))
            .collect();
        let merged: Vec<Corner> = self
            .half_edges_from(after_h)
            .map(|x| self.corner(x))
            .chain(second[1..second.len() - 1].iter().copied())
            .collect();

        let mut verts: Vec<VertId> = merged.iter().map(|c| c.v).collect();
        verts.sort_unstable();
        if verts.windows(2).any(|w| w[0] == w[1]) {
            return Err(MeshError::FacesTouch);
        }
        let loops: Vec<Vec<Corner>> = iter::once(merged)
            .chain(
                self.face_holes(f1)
                    .chain(self.face_holes(f2))
                    .map(|l| self.loop_corners(l).collect()),
            )
            .collect();
        // Test the face that would be built: concatenating the two faces' loops repeats the
        // shared edge, whose contributions cancel in the plane fit.
        if !is_planar(loops.iter().flatten().map(|c| self.position(c.v))) {
            return Err(MeshError::NotCoplanar);
        }
        self.check_new_face(&loops, &[f1, f2])?;
        let material = self.material(f1);

        self.kill_face(f1, RemoveFace::default());
        self.kill_face(f2, RemoveFace::default());
        self.kill_edge(e);
        Ok(self.link_face(Some(f1), &loops, material))
    }

    /// Cuts a hole into a face, bounded by vertices not already in it. The corners may wind
    /// either way; they are reoriented to wind opposite to the face. With [`HoleFill::Filled`],
    /// a new face fills the hole.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`], [`MeshError::NoSuchVertex`], [`MeshError::HoleVertexInFace`],
    /// [`MeshError::NotCoplanar`], or any error from adding the face or the fill.
    pub fn add_hole(
        &mut self,
        f: FaceId,
        corners: &[Corner],
        fill: HoleFill,
    ) -> Result<AddedHole, MeshError> {
        self.check_face(f)?;
        corners.iter().try_for_each(|c| self.check_vert(c.v))?;
        let mut loops = self.face_corners(f);
        let mut face_verts: Vec<VertId> = loops.iter().flatten().map(|c| c.v).collect();
        face_verts.sort_unstable();
        if corners
            .iter()
            .any(|c| face_verts.binary_search(&c.v).is_ok())
        {
            return Err(MeshError::HoleVertexInFace);
        }
        let hole_positions = corners.iter().map(|c| self.position(c.v));
        if !is_planar(self.face_positions(f).chain(hole_positions.clone())) {
            return Err(MeshError::NotCoplanar);
        }
        let face_normal = newell_normal(self.loop_positions(self.face_outer(f)));
        let reverse = face_normal.dot(newell_normal(hole_positions)) > 0.0;
        let hole: Vec<Corner> = if reverse {
            corners.iter().rev().copied().collect()
        } else {
            corners.to_vec()
        };
        let fill_loop: Option<Vec<Corner>> =
            (fill == HoleFill::Filled).then(|| hole.iter().rev().copied().collect());
        loops.push(hole);
        self.check_new_face(&loops, &[f])?;
        if let Some(fill_loop) = &fill_loop {
            self.check_new_face(&[fill_loop], &[f])?;
        }
        // Loops are appended in order, so the hole is the last loop of the rebuilt face.
        let hole = LoopId::from_slot(self.loops.len() + loops.len() - 1);
        let material = self.material(f);

        self.kill_face(f, RemoveFace::default());
        self.link_face(Some(f), &loops, material);
        let fill = fill_loop.map(|l| self.link_face(None, &[l], material));
        Ok(AddedHole { hole, fill })
    }

    /// Creates a face filling a hole of `f`. It winds opposite to the hole and copies its UVs.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoSuchFace`], [`MeshError::NotAHole`], or any error from adding the face
    /// (for example [`MeshError::HalfEdgeInUse`] if the hole is already filled).
    pub fn fill_hole(&mut self, f: FaceId, hole: LoopId) -> Result<FaceId, MeshError> {
        self.check_hole(f, hole)?;
        let mut corners: Vec<Corner> = self.loop_corners(hole).collect();
        corners.reverse();
        let material = self.material(f);
        self.add_face(&[corners], material)
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
            .any(|h| self.loop_of(h.twin()).is_some())
        {
            return Err(MeshError::HoleFilled);
        }
        let hole_verts: Vec<VertId> = self.loop_verts(hole).collect();
        let loops: Vec<Vec<Corner>> = self
            .face_loops(f)
            .filter(|&l| l != hole)
            .map(|l| self.loop_corners(l).collect())
            .collect();
        let material = self.material(f);

        self.kill_face(f, RemoveFace::default());
        self.link_face(Some(f), &loops, material);
        let wire: Vec<EdgeId> = cyclic_pairs(&hole_verts)
            .filter_map(|(a, b)| self.find_edge(a, b))
            .filter(|&e| self.is_wire(e))
            .collect();
        for e in wire {
            self.kill_edge(e);
        }
        for &v in &hole_verts {
            if self.valence(v) == 0 {
                self.kill_vertex(v);
            }
        }
        Ok(())
    }

    fn check_hole(&self, f: FaceId, hole: LoopId) -> Result<(), MeshError> {
        self.check_face(f)?;
        (self.contains_loop(hole) && self.loop_face(hole) == f && self.is_hole(hole))
            .then_some(())
            .ok_or(MeshError::NotAHole(hole))
    }
}

/// `corners` with a corner at `v` inserted wherever the loop crosses the edge `a`-`b`, its UV
/// interpolated at `t` measured from `a`.
fn with_corner_on_edge(corners: &[Corner], [a, b]: [VertId; 2], t: f64, v: VertId) -> Vec<Corner> {
    cyclic_pairs(corners)
        .flat_map(|(c, n)| {
            let crosses = (c.v == a && n.v == b) || (c.v == b && n.v == a);
            let inserted = crosses.then(|| Corner {
                v,
                uv: c.uv.lerp(n.uv, if c.v == a { t } else { 1.0 - t }),
            });
            iter::once(c).chain(inserted)
        })
        .collect()
}
