//! Structural invariant checks.
//!
//! Validation must survive the corruption it exists to report, so every walk is bounded, and it
//! reads raw records rather than the accessors (which assert on dead handles in debug builds).

use glam::{DVec2, DVec3};

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

/// The vertices and positions visited by an intact loop walk.
struct LoopWalk {
    verts: Vec<VertId>,
    positions: Vec<DVec3>,
}

impl EditableMesh {
    /// Checks every structural invariant and returns a description of each problem found
    /// (empty for a valid mesh). Twin and edge relationships hold by construction, so this checks
    /// the links that can go wrong: out lists, loops, faces, UVs and the live counts.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Problems::default();
        self.check_edges(&mut problems);
        self.check_verts(&mut problems);
        let on_loops = self.check_faces(&mut problems);
        self.check_half_edge_links(&on_loops, &mut problems);
        problems.0
    }

    /// Checks the mesh with [`validate`](Self::validate).
    ///
    /// # Errors
    ///
    /// [`MeshError::Invalid`] listing the problems found.
    pub fn check(&self) -> Result<(), MeshError> {
        let errors = self.validate();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(MeshError::Invalid(errors))
        }
    }

    fn raw_origin(&self, h: HalfEdgeId) -> VertId {
        self.half_edges[h.slot()].origin
    }

    /// A vertex's out list, bounded so a cycle can't hang. Yields one extra element past the
    /// number of half-edges if the list doesn't terminate.
    fn raw_out_list(&self, v: VertId) -> impl Iterator<Item = HalfEdgeId> + '_ {
        std::iter::successors(self.verts[v.slot()].first_out, |h| {
            self.half_edges.get(h.slot()).and_then(|r| r.next_out)
        })
        .take(self.half_edges.len() + 1)
    }

    fn check_edges(&self, problems: &mut Problems) {
        if self.half_edges.len() != 2 * self.edges.len() {
            problems.push(format!(
                "{} half-edge slots for {} edge slots",
                self.half_edges.len(),
                self.edges.len()
            ));
            return;
        }
        let mut live_edges = 0;
        for e in self.edges() {
            live_edges += 1;
            let h = e.half_edge();
            let (a, b) = (self.raw_origin(h), self.raw_origin(h.twin()));
            if a == b {
                problems.push(format!(
                    "edge {a}-{b}: both halves start at the same vertex"
                ));
            }
            for (x, v) in [(h, a), (h.twin(), b)] {
                if !self.contains_vert(v) {
                    problems.push(format!("edge {a}-{b}: origin vertex {v} is not live"));
                } else if !self.raw_out_list(v).any(|o| o == x) {
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
            if !self.positions[v.slot()].is_finite() {
                problems.push(format!("vertex {v} has a non-finite position"));
            }
            let out: Vec<HalfEdgeId> = self.raw_out_list(v).collect();
            if out.len() > limit {
                problems.push(format!("vertex {v}: out list does not terminate"));
                continue;
            }
            for &h in &out {
                if !self.contains_half_edge(h) {
                    problems.push(format!(
                        "vertex {v}: out half-edge {h} belongs to a dead edge"
                    ));
                } else if self.raw_origin(h) != v {
                    problems.push(format!(
                        "vertex {v}: out half-edge has origin {}",
                        self.raw_origin(h)
                    ));
                }
            }
            let mut dests: Vec<VertId> = out
                .iter()
                .filter(|&&h| self.contains_half_edge(h))
                .map(|&h| self.raw_origin(h.twin()))
                .collect();
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

    /// Checks faces and their loops. Returns, per half-edge slot, whether an intact loop walk of
    /// a live face visited it.
    fn check_faces(&self, problems: &mut Problems) -> Vec<bool> {
        let mut on_loops = vec![false; self.half_edges.len()];
        let mut live_faces = 0;
        for f in self.faces() {
            live_faces += 1;
            let first = self.faces[f.slot()].outer;
            let loops: Vec<LoopId> = std::iter::successors(Some(first), |l| {
                self.loops.get(l.slot()).and_then(|r| r.next_hole)
            })
            .take(self.loops.len() + 1)
            .collect();
            if loops.len() > self.loops.len() {
                problems.push(format!("face {f}: hole chain does not terminate"));
                continue;
            }
            let walks: Vec<Option<LoopWalk>> = loops
                .iter()
                .map(|&l| self.check_loop(f, l, &mut on_loops, problems))
                .collect();
            let Some(walks) = walks.into_iter().collect::<Option<Vec<LoopWalk>>>() else {
                continue; // a broken loop was reported; its geometry is meaningless
            };

            let mut verts: Vec<VertId> =
                walks.iter().flat_map(|w| w.verts.iter().copied()).collect();
            verts.sort_unstable();
            if let Some(v) = verts
                .windows(2)
                .find_map(|w| (w[0] == w[1]).then_some(w[0]))
            {
                problems.push(format!("face {f}: vertex {v} appears more than once"));
            }
            if !is_planar(walks.iter().flat_map(|w| w.positions.iter().copied())) {
                problems.push(format!("face {f}: not planar"));
            }
            // Holes wind opposite to the outer loop. Degenerate (zero-area) loops are skipped.
            let n = newell_normal(walks[0].positions.iter().copied());
            if walks[1..]
                .iter()
                .any(|hole| n.dot(newell_normal(hole.positions.iter().copied())) > 1e-12)
            {
                problems.push(format!(
                    "face {f}: hole winds the same way as the outer loop"
                ));
            }
        }
        if live_faces != self.live.faces {
            problems.push(format!(
                "{live_faces} live faces, but the count says {}",
                self.live.faces
            ));
        }
        on_loops
    }

    /// Walks one loop of face `f`, marking its half-edges in `on_loops`. Returns `None` (after
    /// reporting why) if the loop is broken.
    fn check_loop(
        &self,
        f: FaceId,
        l: LoopId,
        on_loops: &mut [bool],
        problems: &mut Problems,
    ) -> Option<LoopWalk> {
        let Some(record) = self.loops.get(l.slot()).filter(|r| r.alive) else {
            problems.push(format!("face {f}: loop {l} is dead or unknown"));
            return None;
        };
        if record.face != f {
            problems.push(format!(
                "face {f}: loop {l} belongs to face {}",
                record.face
            ));
        }
        let first = record.first;
        let mut walk = LoopWalk {
            verts: Vec::new(),
            positions: Vec::new(),
        };
        let mut h = first;
        loop {
            if !self.contains_half_edge(h) {
                problems.push(format!(
                    "face {f}: loop {l} contains half-edge {h} of a dead edge"
                ));
                return None;
            }
            if on_loops[h.slot()] {
                problems.push(format!(
                    "face {f}: loop {l} does not close, or shares half-edge {h} with another loop"
                ));
                return None;
            }
            on_loops[h.slot()] = true;
            let r = self.half_edges[h.slot()];
            let (a, b) = (r.origin, self.raw_origin(h.twin()));
            if r.loop_id != Some(l) {
                problems.push(format!("face {f}: half-edge {a}->{b} has the wrong loop"));
            }
            let (Some(next), Some(_)) = (r.next, r.prev) else {
                problems.push(format!("face {f}: half-edge {a}->{b} has no next/prev"));
                return None;
            };
            if !self.contains_half_edge(next) {
                problems.push(format!("face {f}: half-edge {a}->{b} links to a dead edge"));
                return None;
            }
            if self.half_edges[next.slot()].prev != Some(h) {
                problems.push(format!("face {f}: next/prev mismatch at {a}->{b}"));
            }
            if self.raw_origin(next) != b {
                problems.push(format!("face {f}: loop is not continuous at vertex {b}"));
            }
            if !self.uvs[h.slot()].is_finite() {
                problems.push(format!("face {f}: non-finite UV at {a}->{b}"));
            }
            walk.verts.push(a);
            walk.positions.push(self.positions[a.slot()]);
            h = next;
            if h == first {
                break;
            }
        }
        if walk.verts.len() < 3 {
            problems.push(format!("face {f}: loop has {} corners", walk.verts.len()));
        }
        Some(walk)
    }

    /// Every live half-edge is on the loop it claims, or on the boundary with no links and no UV.
    fn check_half_edge_links(&self, on_loops: &[bool], problems: &mut Problems) {
        for e in self.edges() {
            for x in [e.half_edge(), e.half_edge().twin()] {
                let r = self.half_edges[x.slot()];
                let (a, b) = (r.origin, self.raw_origin(x.twin()));
                match r.loop_id {
                    Some(l) if !self.contains_loop(l) => {
                        problems.push(format!("half-edge {a}->{b} references dead loop {l}"));
                    }
                    Some(l) if !on_loops[x.slot()] => {
                        problems.push(format!(
                            "half-edge {a}->{b} claims loop {l} but is not on it"
                        ));
                    }
                    Some(_) => {}
                    None => {
                        if r.next.is_some() || r.prev.is_some() {
                            problems
                                .push(format!("boundary half-edge {a}->{b} has next/prev links"));
                        }
                        if self.uvs[x.slot()] != DVec2::ZERO {
                            problems.push(format!("boundary half-edge {a}->{b} has a UV"));
                        }
                    }
                }
            }
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

    fn first_half_edge(m: &EditableMesh, f: u32) -> HalfEdgeId {
        m.loops[m.face_outer(FaceId::new(f)).slot()].first
    }

    fn joined(m: &EditableMesh) -> String {
        m.validate().join("\n")
    }

    #[track_caller]
    fn assert_reports(m: &EditableMesh, needle: &str) {
        let report = joined(m);
        assert!(report.contains(needle), "expected {needle:?} in:\n{report}");
    }

    #[test]
    fn accepts_valid_meshes() {
        assert_eq!(cube().validate(), Vec::<String>::new());
        assert!(square_with_hole(HoleWinding::Correct).check().is_ok());
    }

    // The TS test "detects broken twin pointers" has no counterpart: twins are `index ^ 1`, so
    // they cannot disagree.

    #[test]
    fn detects_broken_next_prev_links() {
        let mut m = cube();
        let h = first_half_edge(&m, 0);
        let next = m.next(h).unwrap();
        m.half_edges[next.slot()].prev = m.prev(h);
        assert_reports(&m, "next/prev mismatch");
    }

    #[test]
    fn detects_a_loop_that_is_not_continuous() {
        let mut m = cube();
        let h = first_half_edge(&m, 0);
        let skip = m.next(m.next(h).unwrap());
        m.half_edges[h.slot()].next = skip;
        assert_reports(&m, "not continuous");
    }

    #[test]
    fn detects_a_missing_out_list_entry() {
        let mut m = cube();
        let v = VertId::new(0);
        let first = m.verts[v.slot()].first_out.unwrap();
        m.verts[v.slot()].first_out = m.half_edges[first.slot()].next_out;
        assert_reports(&m, "out list");
    }

    #[test]
    fn detects_a_hole_wound_the_same_way_as_the_outer_loop() {
        assert_reports(&square_with_hole(HoleWinding::SameAsOuter), "hole winds");
    }

    #[test]
    fn detects_stale_live_counts() {
        let mut m = cube();
        m.live.verts = 3;
        assert_reports(&m, "live vertices");
        assert!(matches!(m.check(), Err(MeshError::Invalid(_))));
    }

    #[test]
    fn detects_a_non_planar_face() {
        let mut m = cube();
        m.set_position(VertId::new(6), DVec3::new(1.0, 1.0, 1.5));
        assert_reports(&m, "not planar");
    }

    // Rust-only checks and regressions.

    #[test]
    fn terminates_on_a_loop_that_cycles_without_returning_to_its_start() {
        let mut m = cube();
        let h0 = first_half_edge(&m, 0);
        let h1 = m.next(h0).unwrap();
        let h2 = m.next(h1).unwrap();
        m.half_edges[h2.slot()].next = Some(h1); // h0 -> h1 -> h2 -> h1 -> ...
        assert_reports(&m, "does not close");
    }

    #[test]
    fn terminates_on_a_hole_chain_that_cycles() {
        let mut m = square_with_hole(HoleWinding::Correct);
        let hole = m.face_holes(FaceId::new(0)).next().unwrap();
        m.loops[hole.slot()].next_hole = Some(hole);
        assert_reports(&m, "hole chain does not terminate");
    }

    #[test]
    fn detects_a_half_edge_claiming_a_loop_it_is_not_on() {
        let mut m = square_with_hole(HoleWinding::Correct);
        let outer = m.face_outer(FaceId::new(0));
        let boundary = first_half_edge(&m, 0).twin();
        m.half_edges[boundary.slot()].loop_id = Some(outer);
        assert_reports(&m, "claims loop");
    }

    #[test]
    fn detects_a_reference_to_a_dead_loop() {
        let mut m = cube();
        let h = first_half_edge(&m, 0);
        let dead = m.face_outer(FaceId::new(1));
        m.loops[dead.slot()].alive = false;
        m.half_edges[h.twin().slot()].loop_id = Some(dead);
        assert_reports(&m, "dead");
    }

    #[test]
    fn detects_links_and_uvs_on_boundary_half_edges() {
        let mut m = square_with_hole(HoleWinding::Correct);
        let boundary = first_half_edge(&m, 0).twin();
        m.half_edges[boundary.slot()].next = Some(boundary);
        m.uvs[boundary.slot()] = DVec2::ONE;
        assert_reports(&m, "has next/prev links");
        assert_reports(&m, "has a UV");
    }

    #[test]
    fn detects_non_finite_positions_and_uvs() {
        let mut m = cube();
        m.positions[0] = DVec3::NAN;
        let h = first_half_edge(&m, 1);
        m.uvs[h.slot()] = DVec2::new(f64::INFINITY, 0.0);
        assert_reports(&m, "non-finite position");
        assert_reports(&m, "non-finite UV");
    }

    #[test]
    fn detects_duplicate_outgoing_edges() {
        let mut m = cube();
        // A second edge between vertices that are already joined, bypassing add_edge's check.
        m.create_edge(VertId::new(0), VertId::new(1));
        assert_reports(&m, "duplicate outgoing edges");
    }
}
