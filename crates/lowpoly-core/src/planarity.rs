//! Planarity tests. Faces are strictly planar: a polygon counts as planar when every point lies
//! within a small tolerance of its best-fit plane (Newell normal through the centroid). The
//! tolerance scales with the polygon's size, so it is meaningful for small details and large
//! faces alike.

use glam::DVec3;

use crate::math::newell_normal;

/// Allowed deviation from the plane, relative to the polygon's bounding-box diagonal (or 1,
/// whichever is larger).
const RELATIVE_TOLERANCE: f64 = 1e-5;

/// Largest distance of any point from the polygon's best-fit plane. Triangles, and degenerate
/// polygons with no defined plane, deviate by 0.
pub fn planar_deviation(points: &[DVec3]) -> f64 {
    if points.len() <= 3 {
        return 0.0;
    }
    let normal = newell_normal(points).normalize_or_zero();
    if normal == DVec3::ZERO {
        return 0.0;
    }
    let origin = points.iter().sum::<DVec3>() / point_count(points);
    points
        .iter()
        .map(|p| (*p - origin).dot(normal).abs())
        .fold(0.0, f64::max)
}

/// The deviation allowed for this set of points.
pub fn planar_tolerance(points: &[DVec3]) -> f64 {
    let lo = points.iter().copied().fold(DVec3::INFINITY, DVec3::min);
    let hi = points.iter().copied().fold(DVec3::NEG_INFINITY, DVec3::max);
    let diagonal = if points.is_empty() {
        0.0
    } else {
        (hi - lo).length()
    };
    RELATIVE_TOLERANCE * diagonal.max(1.0)
}

/// Whether the points lie in one plane, within [`planar_tolerance`].
pub fn is_planar(points: &[DVec3]) -> bool {
    planar_deviation(points) <= planar_tolerance(points)
}

fn point_count(points: &[DVec3]) -> f64 {
    // Exact for any realistic polygon size.
    #[expect(
        clippy::cast_precision_loss,
        reason = "point counts are far below 2^52"
    )]
    let n = points.len() as f64;
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_polygon_is_planar_and_bent_one_is_not() {
        let mut quad = vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        assert!(is_planar(&quad));
        quad[2].z = 0.01;
        assert!(!is_planar(&quad));
    }

    #[test]
    fn tolerance_scales_with_size() {
        let small = [DVec3::ZERO, DVec3::X];
        let large = [DVec3::ZERO, DVec3::X * 1000.0];
        assert_eq!(planar_tolerance(&small), RELATIVE_TOLERANCE);
        assert!((planar_tolerance(&large) - RELATIVE_TOLERANCE * 1000.0).abs() < 1e-12);
    }
}
