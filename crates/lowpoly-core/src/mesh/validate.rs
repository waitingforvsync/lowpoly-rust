//! Structural invariant checks.

use super::{EditableMesh, FaceId, HalfEdgeId, LoopId, MeshError, VertId};
use crate::math::newell_normal;
use crate::planarity::is_planar;

/// Stop collecting after this many problems; one broken link tends to cause many reports.
const MAX_ERRORS: usize = 50;

/// Collects problem descriptions, up to [`MAX_ERRORS`].
#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn push(&mut self, msg: String) {
        if self.0.len() < MAX_ERRORS {
            self.0.push(msg);
        }
    }
}

impl EditableMesh {
    /// Checks every structural invariant and returns a description of each problem found
    /// (empty for a valid mesh). Twin and edge relationships hold by construction, so this checks
    /// the links that can go wrong: out lists, loops, faces and the live counts.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Problems::default();
        self.check_edges(&mut problems);
        self.check_verts(&mut problems);
        let live_loops = self.check_faces(&mut problems);
        self.check_half_edge_links(&live_loops, &mut problems);
        problems.0
    }

    /// Checks the mesh with [`validate`](Self::validate).
    ///
    /// # Errors
    ///
    /// [`MeshError::Invalid`] listing the problems found.
    pub fn assert_valid(&self) -> Result<(), MeshError> {
        let errors = self.validate();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(MeshError::Invalid(errors))
        }
    }

    fn check_edges(&self, problems: &mut Problems) {
        let limit = self.half_edges.len();
        let mut live_edges = 0;
        for e in self.edges() {
            live_edges += 1;
            let h = self.edge_half_edge(e);
            let [a, b] = self.edge_verts(e);
            if a == b {
                problems.push(format!(
                    "edge {a}-{b}: both halves start at the same vertex"
                ));
            }
            for (x, v) in [(h, a), (self.twin(h), b)] {
                if !self.contains_vert(v) {
                    problems.push(format!("edge {a}-{b}: origin vertex {v} is not live"));
                } else if !self.outgoing(v).take(limit).any(|o| o == x) {
                    problems.push(format!(
                        "edge {a}-{b}: half-edge missing from vertex {v} out list"
                    ));
                }
            }
        }
        if live_edges != self.live.edges {
            problems.push(format!(
                "{live_edges} live edges, but the count says {}",
                self.live.edges
            ));
        }
    }

    fn check_verts(&self, problems: &mut Problems) {
        let limit = self.half_edges.len();
        let mut live_verts = 0;
        let mut out_total = 0;
        for v in self.verts() {
            live_verts += 1;
            if !self.position(v).is_finite() {
                problems.push(format!("vertex {v} has a non-finite position"));
            }
            let out: Vec<HalfEdgeId> = self.outgoing(v).take(limit + 1).collect();
            if out.len() > limit {
                problems.push(format!("vertex {v}: out list does not terminate"));
                continue;
            }
            let mut dests: Vec<VertId> = Vec::with_capacity(out.len());
            for &h in &out {
                if self.origin(h) != v {
                    problems.push(format!(
                        "vertex {v}: out half-edge has origin {}",
                        self.origin(h)
                    ));
                }
                if !self.contains_edge(self.edge_of(h)) {
                    problems.push(format!("vertex {v}: out half-edge belongs to a dead edge"));
                }
                dests.push(self.dest(h));
            }
            dests.sort_unstable();
            if dests.windows(2).any(|w| w[0] == w[1]) {
                problems.push(format!("vertex {v}: duplicate outgoing edges"));
            }
            out_total += out.len();
        }
        if live_verts != self.live.verts {
            problems.push(format!(
                "{live_verts} live vertices, but the count says {}",
                self.live.verts
            ));
        }
        if out_total != 2 * self.live.edges {
            problems.push(format!(
                "vertex out lists hold {out_total} half-edges, expected {}",
                2 * self.live.edges
            ));
        }
    }

    /// Checks faces and their loops, returning the live loops (sorted).
    fn check_faces(&self, problems: &mut Problems) -> Vec<LoopId> {
        let mut live_faces = 0;
        let mut live_loops = Vec::new();
        for f in self.faces() {
            live_faces += 1;
            let mut face_verts = Vec::new();
            for l in self.face_loops(f).take(self.loops.len()) {
                live_loops.push(l);
                self.check_loop(f, l, &mut face_verts, problems);
            }
            if !is_planar(&self.face_positions(f)) {
                problems.push(format!("face {f}: not planar"));
            }
            // Holes wind opposite to the outer loop. Degenerate (zero-area) loops are skipped.
            let n = newell_normal(&self.loop_positions(self.face_outer(f)));
            for hole in self.face_holes(f) {
                if n.dot(newell_normal(&self.loop_positions(hole))) > 1e-12 {
                    problems.push(format!(
                        "face {f}: hole winds the same way as the outer loop"
                    ));
                }
            }
        }
        if live_faces != self.live.faces {
            problems.push(format!(
                "{live_faces} live faces, but the count says {}",
                self.live.faces
            ));
        }
        live_loops.sort_unstable();
        live_loops
    }

    /// Every live half-edge is in a live loop with links, or on the boundary with none.
    fn check_half_edge_links(&self, live_loops: &[LoopId], problems: &mut Problems) {
        for e in self.edges() {
            let h = self.edge_half_edge(e);
            for x in [h, self.twin(h)] {
                let (a, b) = (self.origin(x), self.dest(x));
                match self.loop_of(x) {
                    Some(l) if live_loops.binary_search(&l).is_err() => {
                        problems.push(format!("half-edge {a}->{b} references a dead loop"));
                    }
                    None if self.next(x).is_some() || self.prev(x).is_some() => {
                        problems.push(format!("boundary half-edge {a}->{b} has next/prev links"));
                    }
                    _ => {}
                }
            }
        }
    }

    fn check_loop(
        &self,
        f: FaceId,
        l: LoopId,
        face_verts: &mut Vec<VertId>,
        problems: &mut Problems,
    ) {
        if !self.contains_loop(l) {
            problems.push(format!("face {f}: loop {l} is dead"));
            return;
        }
        if self.loop_face(l) != f {
            problems.push(format!(
                "face {f}: loop {l} belongs to face {}",
                self.loop_face(l)
            ));
        }
        let first = self.loops[l.slot()].first;
        let mut h = first;
        let mut count = 0;
        loop {
            if !self.contains_edge(self.edge_of(h)) {
                problems.push(format!(
                    "face {f}: loop contains a half-edge of a dead edge"
                ));
                return;
            }
            let (a, b) = (self.origin(h), self.dest(h));
            if self.loop_of(h) != Some(l) {
                problems.push(format!("face {f}: half-edge {a}->{b} has the wrong loop"));
            }
            let (Some(next), Some(_)) = (self.next(h), self.prev(h)) else {
                problems.push(format!("face {f}: half-edge {a}->{b} has no next/prev"));
                return;
            };
            if self.prev(next) != Some(h) {
                problems.push(format!("face {f}: next/prev mismatch at {a}->{b}"));
            }
            if self.origin(next) != b {
                problems.push(format!("face {f}: loop is not continuous at vertex {b}"));
            }
            if face_verts.contains(&a) {
                problems.push(format!("face {f}: vertex {a} appears more than once"));
            }
            face_verts.push(a);
            if !self.uv(h).is_finite() {
                problems.push(format!("face {f}: non-finite UV"));
            }
            h = next;
            count += 1;
            if h == first {
                break;
            }
            if count > self.half_edges.len() {
                problems.push(format!("face {f}: loop does not close"));
                return;
            }
        }
        if count < 3 {
            problems.push(format!("face {f}: loop has {count} corners"));
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec2, DVec3};

    use super::super::{Corner, MaterialId};
    use super::*;

    /// Unit cube with outward-facing quads.
    fn cube() -> EditableMesh {
        let mut m = EditableMesh::new();
        let p = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let v: Vec<VertId> = p.iter().map(|&q| m.add_vertex(DVec3::from(q))).collect();
        for quad in [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [0, 4, 7, 3],
            [1, 2, 6, 5],
        ] {
            let corners: Vec<Corner> = quad.iter().map(|&i| Corner::at(v[i])).collect();
            m.add_face(&[corners], MaterialId(0)).unwrap();
        }
        m
    }

    #[derive(Clone, Copy)]
    enum HoleWinding {
        Correct,
        SameAsOuter,
    }

    /// 4x4 square with a 2x2 hole.
    fn square_with_hole(winding: HoleWinding) -> EditableMesh {
        let mut m = EditableMesh::new();
        let mut add = |x: f64, y: f64| Corner {
            v: m.add_vertex(DVec3::new(x, y, 0.0)),
            uv: DVec2::new(x, y) / 4.0,
        };
        let outer = vec![add(0.0, 0.0), add(4.0, 0.0), add(4.0, 4.0), add(0.0, 4.0)];
        let mut hole = vec![add(1.0, 1.0), add(1.0, 3.0), add(3.0, 3.0), add(3.0, 1.0)];
        if matches!(winding, HoleWinding::SameAsOuter) {
            hole.reverse();
        }
        m.add_face(&[outer, hole], MaterialId(0)).unwrap();
        m
    }

    fn joined(m: &EditableMesh) -> String {
        m.validate().join("\n")
    }

    #[test]
    fn accepts_valid_meshes() {
        assert_eq!(cube().validate(), Vec::<String>::new());
        assert!(
            square_with_hole(HoleWinding::Correct)
                .assert_valid()
                .is_ok()
        );
    }

    // The TS test "detects broken twin pointers" has no counterpart: twins are `index ^ 1`, so
    // they cannot disagree.

    #[test]
    fn detects_broken_next_prev_links() {
        let mut m = cube();
        let h = m.loops[m.face_outer(FaceId::new(0)).slot()].first;
        let next = m.next(h).unwrap();
        m.half_edges[next.slot()].prev = m.prev(h);
        assert!(joined(&m).contains("next/prev mismatch"), "{}", joined(&m));
    }

    #[test]
    fn detects_a_loop_that_is_not_continuous() {
        let mut m = cube();
        let h = m.loops[m.face_outer(FaceId::new(0)).slot()].first;
        let skip = m.next(m.next(h).unwrap());
        m.half_edges[h.slot()].next = skip;
        assert_ne!(m.validate(), Vec::<String>::new());
    }

    #[test]
    fn detects_a_missing_out_list_entry() {
        let mut m = cube();
        let v = VertId::new(0);
        let first = m.verts[v.slot()].first_out.unwrap();
        m.verts[v.slot()].first_out = m.half_edges[first.slot()].next_out;
        assert!(joined(&m).contains("out list"), "{}", joined(&m));
    }

    #[test]
    fn detects_a_hole_wound_the_same_way_as_the_outer_loop() {
        let m = square_with_hole(HoleWinding::SameAsOuter);
        assert!(joined(&m).contains("hole winds"), "{}", joined(&m));
    }

    #[test]
    fn detects_stale_live_counts() {
        let mut m = cube();
        m.live.verts = 3;
        assert!(joined(&m).contains("live vertices"), "{}", joined(&m));
        assert!(matches!(m.assert_valid(), Err(MeshError::Invalid(_))));
    }

    #[test]
    fn detects_a_non_planar_face() {
        let mut m = cube();
        m.set_position(VertId::new(6), DVec3::new(1.0, 1.0, 1.5));
        assert!(joined(&m).contains("not planar"), "{}", joined(&m));
    }
}
