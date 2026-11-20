//! Triangulation of faces with holes, for rendering and export only (faces are n-gons).

use std::fmt;

use earcut::Earcut;
use glam::{DVec2, DVec3};

use crate::math::{newell_normal, plane_basis};
use crate::mesh::{EditableMesh, FaceId, HalfEdgeId};

/// Triangulates polygons with holes, reusing its buffers between calls.
///
/// Each polygon is projected onto the best-fit plane of its outer loop (Newell normal), so
/// mildly non-planar input still triangulates sensibly. Every triangle winds counter-clockwise
/// about the outer loop's normal.
#[derive(Default)]
pub struct Triangulator {
    earcut: Earcut<f64>,
    /// Points of the polygon being triangulated, flattened over its loops.
    points: Vec<DVec3>,
    /// Start index of each hole in `points`.
    holes: Vec<u32>,
    /// The points projected onto the polygon's plane.
    coords: Vec<[f64; 2]>,
    /// Triangles of the current polygon, before offsetting into the caller's buffer.
    triangles: Vec<u32>,
}

impl fmt::Debug for Triangulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Triangulator").finish_non_exhaustive()
    }
}

impl Triangulator {
    /// Creates a triangulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Triangulates `loops` (outer boundary first, then holes) into `out`, as indices into the
    /// loops' points flattened in order. Replaces the contents of `out`.
    ///
    /// A degenerate outer loop (for example collinear points) with no holes becomes a fan, so
    /// the face still exists for picking and export.
    pub fn triangulate(&mut self, loops: &[impl AsRef<[DVec3]>], out: &mut Vec<u32>) {
        self.points.clear();
        self.holes.clear();
        for (i, points) in loops.iter().map(AsRef::as_ref).enumerate() {
            if i > 0 {
                self.holes.push(index(self.points.len()));
            }
            self.points.extend_from_slice(points);
        }
        let outer_len = loops.first().map_or(0, |l| l.as_ref().len());
        self.run(outer_len);
        out.clear();
        out.extend_from_slice(&self.triangles);
    }

    /// Triangulates face `f` of `mesh`, appending its corners (half-edges, over the outer loop
    /// then holes) to `corners` and its triangles to `indices`, as indices into `corners`. Lets a
    /// caller build one buffer pair for a whole mesh without per-face allocation.
    pub fn triangulate_face_into(
        &mut self,
        mesh: &EditableMesh,
        f: FaceId,
        corners: &mut Vec<HalfEdgeId>,
        indices: &mut Vec<u32>,
    ) {
        let base = corners.len();
        self.points.clear();
        self.holes.clear();
        let mut outer_len = 0;
        for (i, l) in mesh.face_loops(f).enumerate() {
            if i > 0 {
                self.holes.push(index(self.points.len()));
            }
            let start = corners.len();
            corners.extend(mesh.loop_half_edges(l));
            self.points.extend(
                corners[start..]
                    .iter()
                    .map(|&h| mesh.position(mesh.origin(h))),
            );
            if i == 0 {
                outer_len = corners.len() - start;
            }
        }
        self.run(outer_len);
        let offset = index(base);
        indices.extend(self.triangles.iter().map(|&i| i + offset));
    }

    /// Triangulates face `f` of `mesh` into fresh buffers.
    pub fn triangulate_face(&mut self, mesh: &EditableMesh, f: FaceId) -> FaceTriangulation {
        let mut corners = Vec::new();
        let mut indices = Vec::new();
        self.triangulate_face_into(mesh, f, &mut corners, &mut indices);
        FaceTriangulation { corners, indices }
    }

    /// Triangulates `self.points` (outer loop of `outer_len` points, then the holes starting at
    /// `self.holes`) into `self.triangles`.
    fn run(&mut self, outer_len: usize) {
        self.triangles.clear();
        if outer_len < 3 {
            return;
        }
        let n = newell_normal(self.points[..outer_len].iter().copied()).normalize_or_zero();
        let (u, v) = plane_basis(if n == DVec3::ZERO { DVec3::Z } else { n });
        self.coords.clear();
        self.coords
            .extend(self.points.iter().map(|p| [p.dot(u), p.dot(v)]));
        self.earcut
            .earcut_slice(&self.coords, &self.holes, &mut self.triangles);

        if self.triangles.is_empty() && self.holes.is_empty() {
            let last = index(outer_len) - 1;
            self.triangles.extend((1..last).flat_map(|i| [0, i, i + 1]));
            return;
        }

        // Make every triangle counter-clockwise in the plane (u × v = n, so CCW in 2D is CCW
        // about n).
        for tri in self.triangles.as_chunks_mut::<3>().0 {
            let [a, b, c] = tri.map(|i| DVec2::from(self.coords[i as usize]));
            if (b - a).perp_dot(c - a) < 0.0 {
                tri.swap(1, 2);
            }
        }
    }
}

/// Triangulates a polygon with holes; see [`Triangulator::triangulate`].
pub fn triangulate_polygon(loops: &[impl AsRef<[DVec3]>]) -> Vec<u32> {
    let mut out = Vec::new();
    Triangulator::new().triangulate(loops, &mut out);
    out
}

/// The triangles of one face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceTriangulation {
    /// The face's corners, flattened over the outer loop then holes. Each half-edge gives a
    /// corner's position (its origin) and UV.
    pub corners: Vec<HalfEdgeId>,
    /// Triangle indices into `corners`.
    pub indices: Vec<u32>,
}

fn index(i: usize) -> u32 {
    u32::try_from(i).expect("polygons have fewer than 2^32 points")
}
