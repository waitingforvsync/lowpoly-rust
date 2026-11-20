//! Planarity tests. Faces are strictly planar: a polygon counts as planar when every point lies
//! within a small tolerance of its best-fit plane (Newell normal through the centroid). The
//! tolerance scales with the polygon's size, so it is meaningful for small details and large
//! faces alike.
//!
//! The functions take any cloneable iterator of points, so callers can pass mesh queries without
//! collecting them.

use glam::DVec3;

use crate::math::newell_normal;

/// Allowed deviation from the plane, relative to the polygon's bounding-box diagonal (or 1,
/// whichever is larger).
const RELATIVE_TOLERANCE: f64 = 1e-5;

/// Below this fraction of the squared extent, a Newell normal is treated as degenerate.
const DEGENERATE_NORMAL: f64 = 1e-9;

/// Largest distance of any point from the points' best-fit plane. Three or fewer points, and
/// collinear points, deviate by 0. Any non-finite point makes it infinite.
///
/// The plane normal is the Newell normal of the points taken as a polygon. When that cancels out
/// (a self-overlapping sequence, such as two faces' loops concatenated, can sum to zero even when
/// the points are far from coplanar), it falls back to the normal of the widest triangle.
pub fn planar_deviation<I>(points: I) -> f64
where
    I: IntoIterator<Item = DVec3>,
    I::IntoIter: Clone,
{
    let points = points.into_iter();
    let (count, sum) = points
        .clone()
        .fold((0_usize, DVec3::ZERO), |(n, sum), p| (n + 1, sum + p));
    if points.clone().any(|p| !p.is_finite()) {
        return f64::INFINITY;
    }
    if count <= 3 {
        return 0.0;
    }
    let extent = bounding_diagonal(points.clone());
    let newell = newell_normal(points.clone());
    let normal = if newell.length() > DEGENERATE_NORMAL * extent * extent {
        newell.normalize()
    } else {
        widest_triangle_normal(points.clone())
    };
    if normal == DVec3::ZERO {
        return 0.0; // collinear
    }
    let origin = sum / to_f64(count);
    points
        .map(|p| (p - origin).dot(normal).abs())
        .fold(0.0, f64::max)
}

/// The unit normal of a well-spread triangle through the points: the first point, the point
/// farthest from it, and the point farthest from the line through those two. Zero if the points
/// are collinear.
fn widest_triangle_normal(points: impl Iterator<Item = DVec3> + Clone) -> DVec3 {
    let mut rest = points.clone();
    let Some(first) = rest.next() else {
        return DVec3::ZERO;
    };
    let farthest = points
        .clone()
        .max_by(|a, b| {
            a.distance_squared(first)
                .total_cmp(&b.distance_squared(first))
        })
        .unwrap_or(first);
    let axis = farthest - first;
    points
        .map(|p| axis.cross(p - first))
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
        .map_or(DVec3::ZERO, DVec3::normalize_or_zero)
}

fn bounding_diagonal(points: impl Iterator<Item = DVec3>) -> f64 {
    points
        .fold(None, |bounds: Option<(DVec3, DVec3)>, p| {
            Some(bounds.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))))
        })
        .map_or(0.0, |(lo, hi)| (hi - lo).length())
}

/// The deviation allowed for this set of points.
pub fn planar_tolerance(points: impl IntoIterator<Item = DVec3>) -> f64 {
    RELATIVE_TOLERANCE * bounding_diagonal(points.into_iter()).max(1.0)
}

/// Whether the points lie in one plane, within [`planar_tolerance`]. Non-finite points never do.
pub fn is_planar<I>(points: I) -> bool
where
    I: IntoIterator<Item = DVec3>,
    I::IntoIter: Clone,
{
    let points = points.into_iter();
    // An infinite point also makes the tolerance infinite, so compare only finite deviations.
    let deviation = planar_deviation(points.clone());
    deviation.is_finite() && deviation <= planar_tolerance(points)
}

fn to_f64(count: usize) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "point counts are far below 2^52, where the conversion is exact"
    )]
    let n = count as f64;
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_polygon_is_planar_and_bent_one_is_not() {
        let mut quad = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        assert!(is_planar(quad.iter().copied()));
        quad[2].z = 0.01;
        assert!(!is_planar(quad.iter().copied()));
    }

    #[test]
    fn non_finite_points_are_never_planar() {
        for bad in [f64::NAN, f64::INFINITY] {
            let quad = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, bad), DVec3::Y];
            assert!(!is_planar(quad), "z = {bad}");
            let triangle = [DVec3::ZERO, DVec3::X, DVec3::new(bad, 1.0, 0.0)];
            assert!(!is_planar(triangle), "triangle with x = {bad}");
        }
    }

    #[test]
    fn self_cancelling_sequences_are_still_tested() {
        // Two triangles on different sides of a cube, concatenated with their shared edge
        // repeated: every edge appears in both directions, so the Newell normal is zero.
        let (a, b) = (DVec3::new(0.0, 1.0, 1.0), DVec3::new(0.0, 0.0, 1.0));
        let points = [
            a,
            b,
            DVec3::new(0.5, 0.5, 1.0),
            b,
            a,
            DVec3::new(0.0, 1.0, 0.05),
        ];
        assert_eq!(newell_normal(points), DVec3::ZERO);
        assert!(!is_planar(points));
        assert!(
            is_planar([DVec3::ZERO, DVec3::X, DVec3::X * 2.0, DVec3::X * 3.0]),
            "collinear"
        );
    }

    #[test]
    fn tolerance_scales_with_size() {
        let small = [DVec3::ZERO, DVec3::X];
        let large = [DVec3::ZERO, DVec3::X * 1000.0];
        assert!((planar_tolerance(small) - RELATIVE_TOLERANCE).abs() < 1e-18);
        assert!((planar_tolerance(large) - RELATIVE_TOLERANCE * 1000.0).abs() < 1e-12);
    }
}
