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
    coords: Vec<[f64; 2]>,
    holes: Vec<u32>,
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
        out.clear();
        let Some(outer) = loops.first().map(AsRef::as_ref) else {
            return;
        };
        if outer.len() < 3 {
            return;
        }
        let n = newell_normal(outer).normalize_or_zero();
        let (u, v) = plane_basis(if n == DVec3::ZERO { DVec3::Z } else { n });

        self.coords.clear();
        self.holes.clear();
        for (i, l) in loops.iter().enumerate() {
            if i > 0 {
                self.holes.push(index(self.coords.len()));
            }
            self.coords
                .extend(l.as_ref().iter().map(|p| [p.dot(u), p.dot(v)]));
        }
        self.earcut
            .earcut(self.coords.iter().copied(), &self.holes, out);

        if out.is_empty() && loops.len() == 1 {
            out.extend((1..index(outer.len()) - 1).flat_map(|i| [0, i, i + 1]));
            return;
        }

        // Make every triangle counter-clockwise in the plane (u × v = n, so CCW in 2D is CCW
        // about n).
        for tri in out.as_chunks_mut::<3>().0 {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| DVec2::from(self.coords[i as usize]));
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
#[derive(Clone, Debug, PartialEq)]
pub struct FaceTriangulation {
    /// The face's corners, flattened over the outer loop then holes. Each half-edge gives a
    /// corner's position (its origin) and UV.
    pub corners: Vec<HalfEdgeId>,
    /// Triangle indices into `corners`.
    pub indices: Vec<u32>,
}

impl Triangulator {
    /// Triangulates a face of `mesh`.
    pub fn triangulate_face(&mut self, mesh: &EditableMesh, f: FaceId) -> FaceTriangulation {
        let loops: Vec<Vec<HalfEdgeId>> = mesh
            .face_loops(f)
            .map(|l| mesh.loop_half_edges(l).collect())
            .collect();
        let positions: Vec<Vec<DVec3>> = loops
            .iter()
            .map(|l| l.iter().map(|&h| mesh.position(mesh.origin(h))).collect())
            .collect();
        let mut indices = Vec::new();
        self.triangulate(&positions, &mut indices);
        FaceTriangulation {
            corners: loops.into_iter().flatten().collect(),
            indices,
        }
    }
}

fn index(i: usize) -> u32 {
    u32::try_from(i).expect("polygons have fewer than 2^32 points")
}
