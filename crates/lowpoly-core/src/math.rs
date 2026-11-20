//! Polygon geometry helpers on top of glam.

use glam::{DVec2, DVec3};

/// Consecutive pairs around a closed polygon: `(p0, p1), (p1, p2), …, (pn-1, p0)`.
pub(crate) fn cyclic_pairs<T: Copy>(items: &[T]) -> impl Iterator<Item = (T, T)> + Clone + '_ {
    items
        .iter()
        .copied()
        .zip(items.iter().copied().cycle().skip(1))
}

/// Newell's method: the area-weighted normal of a (possibly non-planar) polygon, with length
/// equal to twice its area. Counter-clockwise winding (right-hand rule) points it towards the
/// viewer. Takes the points in order; the polygon closes back to the first.
pub fn newell_normal(points: impl IntoIterator<Item = DVec3>) -> DVec3 {
    let mut points = points.into_iter();
    let Some(first) = points.next() else {
        return DVec3::ZERO;
    };
    let (sum, last) = points.fold((DVec3::ZERO, first), |(sum, prev), p| {
        (sum + newell_term(prev, p), p)
    });
    sum + newell_term(last, first)
}

/// One edge's contribution to the Newell normal.
fn newell_term(a: DVec3, b: DVec3) -> DVec3 {
    DVec3::new(
        (a.y - b.y) * (a.z + b.z),
        (a.z - b.z) * (a.x + b.x),
        (a.x - b.x) * (a.y + b.y),
    )
}

/// An orthonormal basis `(u, v)` spanning the plane with unit normal `n`, with `u × v = n`.
pub fn plane_basis(n: DVec3) -> (DVec3, DVec3) {
    // Build u from the world axis least aligned with n.
    let a = n.abs();
    let helper = if a.x <= a.y && a.x <= a.z {
        DVec3::X
    } else if a.y <= a.z {
        DVec3::Y
    } else {
        DVec3::Z
    };
    let u = helper.cross(n).normalize_or_zero();
    (u, n.cross(u))
}

/// Projects 3D points onto 2D coordinates in a polygon's plane. Counter-clockwise about the
/// polygon's normal stays counter-clockwise in 2D.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneProjector {
    u: DVec3,
    v: DVec3,
}

impl PlaneProjector {
    /// A projector onto the best-fit plane of the polygon `outer` (its Newell normal, or +Z if
    /// the polygon is degenerate).
    pub fn new(outer: impl IntoIterator<Item = DVec3>) -> Self {
        let n = newell_normal(outer).normalize_or_zero();
        let (u, v) = plane_basis(if n == DVec3::ZERO { DVec3::Z } else { n });
        Self { u, v }
    }

    /// The point's coordinates in the plane.
    pub fn project(&self, p: DVec3) -> DVec2 {
        DVec2::new(p.dot(self.u), p.dot(self.v))
    }
}

/// Signed area of a 2D polygon, positive when counter-clockwise.
pub fn signed_area_2d(points: &[DVec2]) -> f64 {
    cyclic_pairs(points)
        .map(|(p, q)| p.perp_dot(q))
        .sum::<f64>()
        / 2.0
}

/// Even-odd point-in-polygon test in 2D.
pub fn point_in_polygon_2d(p: DVec2, polygon: &[DVec2]) -> bool {
    let crosses = |(a, b): &(DVec2, DVec2)| {
        (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
    };
    cyclic_pairs(polygon).filter(crosses).count() % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyclic_pairs_wrap_around() {
        assert_eq!(
            cyclic_pairs(&[1, 2, 3]).collect::<Vec<_>>(),
            [(1, 2), (2, 3), (3, 1)]
        );
        assert_eq!(cyclic_pairs::<i32>(&[]).count(), 0);
    }

    #[test]
    fn newell_normal_points_up_for_ccw_square_with_length_twice_area() {
        let n = newell_normal([
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(2.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ]);
        assert_eq!(n, DVec3::new(0.0, 0.0, 8.0));
    }

    #[test]
    fn newell_normal_flips_with_winding() {
        let n = newell_normal([DVec3::ZERO, DVec3::Y, DVec3::X]);
        assert!(n.z < 0.0, "{n}");
    }

    #[test]
    fn newell_normal_of_nothing_is_zero() {
        assert_eq!(newell_normal([]), DVec3::ZERO);
    }

    #[test]
    fn plane_basis_is_orthonormal_and_right_handed() {
        let normals = [
            DVec3::Z,
            DVec3::NEG_Z,
            DVec3::X,
            DVec3::Y,
            DVec3::new(1.0, 2.0, 3.0).normalize(),
            DVec3::new(-0.3, 0.1, -0.9).normalize(),
        ];
        for n in normals {
            let (u, v) = plane_basis(n);
            assert!(
                (u.length() - 1.0).abs() < 1e-12,
                "{n}: |u| = {}",
                u.length()
            );
            assert!(
                (v.length() - 1.0).abs() < 1e-12,
                "{n}: |v| = {}",
                v.length()
            );
            assert!(u.dot(v).abs() < 1e-12, "{n}: u·v");
            assert!(u.dot(n).abs() < 1e-12, "{n}: u·n");
            assert!(
                u.cross(v).abs_diff_eq(n, 1e-12),
                "{n}: u×v = {}",
                u.cross(v)
            );
        }
    }

    #[test]
    fn signed_area_is_positive_counter_clockwise() {
        let mut square = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        assert!((signed_area_2d(&square) - 4.0).abs() < 1e-12);
        square.reverse();
        assert!((signed_area_2d(&square) + 4.0).abs() < 1e-12);
    }

    #[test]
    fn point_in_polygon_tests_containment() {
        let square = [
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        assert!(point_in_polygon_2d(DVec2::new(1.0, 1.0), &square));
        assert!(!point_in_polygon_2d(DVec2::new(3.0, 1.0), &square));
    }
}
