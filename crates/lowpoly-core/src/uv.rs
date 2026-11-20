//! UV generation. UVs are in world units: 1 UV unit = 1 length unit.

use glam::{DVec2, DVec3};

use crate::math::{newell_normal, plane_basis};

/// Relative tolerance for degeneracy tests. Each test compares against a quantity of the same
/// dimension built from the input, so small faces behave like large ones.
const REL_EPS: f64 = 1e-9;

/// UV axes for planar projection onto a face with the given normal. Vertical-ish faces use +Z
/// as "up" so textures stand upright, horizontal faces use +Y. Textures read correctly (not
/// mirrored) when the face is viewed from the front.
pub fn planar_axes(normal: DVec3) -> (DVec3, DVec3) {
    let n = normal.normalize_or_zero();
    let up = if n.z.abs() > 0.9 { DVec3::Y } else { DVec3::Z };
    let u = up.cross(n).normalize_or_zero();
    (u, n.cross(u))
}

/// Projects a point onto planar UV axes.
pub fn planar_uv(p: DVec3, (u, v): (DVec3, DVec3)) -> DVec2 {
    DVec2::new(round(p.dot(u)), round(p.dot(v)))
}

/// An affine map from points in a face's plane to UVs. See [`face_uv_map`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AffineUvMap {
    /// Plane basis.
    axes: (DVec3, DVec3),
    /// Centroid of the fitted points, in plane coordinates.
    mean_xy: DVec2,
    /// Centroid of the fitted UVs.
    mean_uv: DVec2,
    /// Rows of the linear part: `uv - mean_uv = (row_u · d, row_v · d)` for plane offset `d`.
    row_u: DVec2,
    row_v: DVec2,
}

impl AffineUvMap {
    /// The UV at a point in the face's plane.
    pub fn uv(&self, p: DVec3) -> DVec2 {
        let d = DVec2::new(p.dot(self.axes.0), p.dot(self.axes.1)) - self.mean_xy;
        DVec2::new(
            round(self.mean_uv.x + self.row_u.dot(d)),
            round(self.mean_uv.y + self.row_v.dot(d)),
        )
    }
}

/// The affine map from a planar face's surface to its UVs, least-squares fitted to its corners.
/// Evaluating it at a new point in the face's plane continues the face's texture exactly.
///
/// Returns `None` when there is no mapping to continue: fewer than three corners, mismatched
/// lengths, a degenerate polygon, or degenerate corner UVs (for example all equal).
pub fn face_uv_map(points: &[DVec3], uvs: &[DVec2]) -> Option<AffineUvMap> {
    if points.len() < 3 || uvs.len() != points.len() {
        return None;
    }
    let n = newell_normal(points.iter().copied());
    let extent = bounding_diagonal(points);
    if n.length() <= REL_EPS * extent * extent {
        return None;
    }
    let axes = plane_basis(n.normalize());
    let plane: Vec<DVec2> = points
        .iter()
        .map(|p| DVec2::new(p.dot(axes.0), p.dot(axes.1)))
        .collect();
    let count = DVec2::splat(f64::from(u32::try_from(points.len()).ok()?));
    let mean_xy = plane.iter().sum::<DVec2>() / count;
    let mean_uv = uvs.iter().sum::<DVec2>() / count;

    // Normal equations of the least-squares fit, accumulated in one pass.
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    let (mut sxu, mut syu, mut sxv, mut syv) = (0.0, 0.0, 0.0, 0.0);
    for (q, uv) in plane.iter().zip(uvs) {
        let d = *q - mean_xy;
        let duv = *uv - mean_uv;
        sxx += d.x * d.x;
        sxy += d.x * d.y;
        syy += d.y * d.y;
        sxu += d.x * duv.x;
        syu += d.y * duv.x;
        sxv += d.x * duv.y;
        syv += d.y * duv.y;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() <= REL_EPS * (sxx + syy) * (sxx + syy) {
        return None;
    }
    let row_u = DVec2::new(syy * sxu - sxy * syu, sxx * syu - sxy * sxu) / det;
    let row_v = DVec2::new(syy * sxv - sxy * syv, sxx * syv - sxy * sxv) / det;
    // A UV-degenerate face (every corner at the same UV) has no mapping worth continuing.
    let scale = row_u.length_squared() + row_v.length_squared();
    if scale == 0.0 || row_u.perp_dot(row_v).abs() <= REL_EPS * scale {
        return None;
    }
    Some(AffineUvMap {
        axes,
        mean_xy,
        mean_uv,
        row_u,
        row_v,
    })
}

/// How to generate UVs for new points in a face's plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UvMapping {
    /// Continue the face's own texture.
    Affine(AffineUvMap),
    /// World-space planar projection onto these axes.
    Planar((DVec3, DVec3)),
}

impl UvMapping {
    /// The UV at a point in the face's plane.
    pub fn uv(&self, p: DVec3) -> DVec2 {
        match self {
            Self::Affine(map) => map.uv(p),
            Self::Planar(axes) => planar_uv(p, *axes),
        }
    }
}

/// UVs for new points in a face's plane: continue its texture ([`face_uv_map`]), or, when its
/// UVs are degenerate, fall back to a world-space planar projection.
pub fn face_uv_mapping(points: &[DVec3], uvs: &[DVec2]) -> UvMapping {
    face_uv_map(points, uvs).map_or_else(
        || {
            let n = newell_normal(points.iter().copied());
            let extent = bounding_diagonal(points);
            let normal = if n.length() <= REL_EPS * extent * extent {
                DVec3::Z
            } else {
                n
            };
            UvMapping::Planar(planar_axes(normal))
        },
        UvMapping::Affine,
    )
}

/// Length of the bounding box diagonal (0 for no points).
fn bounding_diagonal(points: &[DVec3]) -> f64 {
    points
        .iter()
        .copied()
        .fold(None, |bounds: Option<(DVec3, DVec3)>, p| {
            Some(bounds.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))))
        })
        .map_or(0.0, |(lo, hi)| (hi - lo).length())
}

/// Trims floating-point noise so generated UVs come out as clean numbers. Rounds halves up, as
/// JavaScript's `Math.round` does, so results match the TS app.
fn round(x: f64) -> f64 {
    // `+ 0.0` turns -0.0 into 0.0.
    round_half_up(x * 1e9) / 1e9 + 0.0
}

/// Rounds to the nearest integer, halves towards +infinity (JavaScript's `Math.round`).
fn round_half_up(x: f64) -> f64 {
    (x + 0.5).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(side: f64) -> [DVec3; 4] {
        [
            DVec3::ZERO,
            DVec3::new(side, 0.0, 0.0),
            DVec3::new(side, side, 0.0),
            DVec3::new(0.0, side, 0.0),
        ]
    }

    #[test]
    fn face_uv_map_continues_an_affine_mapping() {
        // UV = (x/4, y/2) on a quad in the XY plane.
        let points = [
            DVec3::ZERO,
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ];
        let uvs: Vec<DVec2> = points
            .iter()
            .map(|p| DVec2::new(p.x / 4.0, p.y / 2.0))
            .collect();
        let map = face_uv_map(&points, &uvs).expect("a mapping");
        assert_eq!(map.uv(DVec3::new(2.0, 1.0, 0.0)), DVec2::new(0.5, 0.5));
        assert_eq!(map.uv(DVec3::new(8.0, -2.0, 0.0)), DVec2::new(2.0, -1.0));
    }

    #[test]
    fn small_faces_still_continue_their_texture() {
        // A 5 mm square with UV = 100 · xy.
        let points = quad(0.005);
        let uvs: Vec<DVec2> = points.iter().map(|p| p.truncate() * 100.0).collect();
        let map = face_uv_map(&points, &uvs).expect("small faces keep their mapping");
        let uv = map.uv(DVec3::new(0.0025, 0.001, 0.0));
        assert!(uv.abs_diff_eq(DVec2::new(0.25, 0.1), 1e-9), "{uv}");
    }

    #[test]
    fn degenerate_uvs_fall_back_to_planar_projection() {
        let points = quad(1.0);
        let uvs = [DVec2::ZERO; 4];
        assert!(face_uv_map(&points, &uvs).is_none());
        let mapping = face_uv_mapping(&points, &uvs);
        assert!(matches!(mapping, UvMapping::Planar(_)));
        // Horizontal face: u along +X, v along +Y.
        assert_eq!(
            mapping.uv(DVec3::new(0.25, 0.75, 0.0)),
            DVec2::new(0.25, 0.75)
        );
    }

    #[test]
    fn planar_axes_keep_walls_upright() {
        let (u, v) = planar_axes(DVec3::NEG_Y);
        assert!(u.abs_diff_eq(DVec3::X, 1e-12), "u = {u}");
        assert!(v.abs_diff_eq(DVec3::Z, 1e-12), "v = {v}");
    }

    #[test]
    fn rounding_matches_javascript_on_negative_halves() {
        // Math.round(-0.5) is -0, Math.round(-1.5) is -1, Math.round(2.5) is 3.
        assert_eq!(round_half_up(-0.5), 0.0);
        assert_eq!(round_half_up(-1.5), -1.0);
        assert_eq!(round_half_up(2.5), 3.0);
        assert_eq!(round(-0.0).to_bits(), 0.0_f64.to_bits(), "no negative zero");
    }
}
