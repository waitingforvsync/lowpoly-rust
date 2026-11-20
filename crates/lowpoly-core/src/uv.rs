//! UV generation. UVs are in world units: 1 UV unit = 1 length unit.

use glam::{DVec2, DVec3};

use crate::math::{newell_normal, plane_basis};

/// Below this, a normal or determinant counts as degenerate.
const EPS: f64 = 1e-9;

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

/// The affine map from a planar face's surface to its UVs, least-squares fitted to its corners.
/// Evaluating it at a new point in the face's plane continues the face's texture exactly.
///
/// Returns `None` when there is no mapping to continue: a degenerate polygon, or corner UVs
/// that are degenerate (for example all equal).
pub fn face_uv_map(points: &[DVec3], uvs: &[DVec2]) -> Option<impl Fn(DVec3) -> DVec2 + use<>> {
    let n = newell_normal(points);
    if n.length() < EPS || points.len() < 3 || uvs.len() != points.len() {
        return None;
    }
    let (u, v) = plane_basis(n.normalize());
    let plane: Vec<DVec2> = points
        .iter()
        .map(|p| DVec2::new(p.dot(u), p.dot(v)))
        .collect();
    let count = DVec2::splat(f64::from(u32::try_from(points.len()).ok()?));
    let mean_xy = plane.iter().sum::<DVec2>() / count;
    let mean_uv = uvs.iter().sum::<DVec2>() / count;

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
    if det.abs() < EPS {
        return None;
    }
    let a = (syy * sxu - sxy * syu) / det;
    let b = (sxx * syu - sxy * sxu) / det;
    let c = (syy * sxv - sxy * syv) / det;
    let d = (sxx * syv - sxy * sxv) / det;
    // A UV-degenerate face (every corner at the same UV) has no mapping worth continuing.
    if (a * d - b * c).abs() < EPS {
        return None;
    }
    Some(move |p: DVec3| {
        let x = p.dot(u) - mean_xy.x;
        let y = p.dot(v) - mean_xy.y;
        DVec2::new(
            round(mean_uv.x + a * x + b * y),
            round(mean_uv.y + c * x + d * y),
        )
    })
}

/// UVs for new points in a face's plane: continues its texture ([`face_uv_map`]), or, when its
/// UVs are degenerate, falls back to a world-space planar projection.
pub fn face_uv_function(points: &[DVec3], uvs: &[DVec2]) -> Box<dyn Fn(DVec3) -> DVec2> {
    if let Some(map) = face_uv_map(points, uvs) {
        return Box::new(map);
    }
    let n = newell_normal(points);
    let axes = planar_axes(if n.length() < EPS { DVec3::Z } else { n });
    Box::new(move |p| planar_uv(p, axes))
}

/// Trims floating-point noise so generated UVs come out as clean numbers.
fn round(x: f64) -> f64 {
    // `+ 0.0` turns -0.0 into 0.0.
    (x * 1e9).round() / 1e9 + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(map(DVec3::new(2.0, 1.0, 0.0)), DVec2::new(0.5, 0.5));
        assert_eq!(map(DVec3::new(8.0, -2.0, 0.0)), DVec2::new(2.0, -1.0));
    }

    #[test]
    fn degenerate_uvs_fall_back_to_planar_projection() {
        let points = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        let uvs = [DVec2::ZERO; 4];
        assert!(face_uv_map(&points, &uvs).is_none());
        let f = face_uv_function(&points, &uvs);
        // Horizontal face: u along +X, v along +Y.
        assert_eq!(f(DVec3::new(0.25, 0.75, 0.0)), DVec2::new(0.25, 0.75));
    }

    #[test]
    fn planar_axes_keep_walls_upright() {
        let (u, v) = planar_axes(DVec3::NEG_Y);
        assert!(u.abs_diff_eq(DVec3::X, 1e-12), "u = {u}");
        assert!(v.abs_diff_eq(DVec3::Z, 1e-12), "v = {v}");
    }
}
