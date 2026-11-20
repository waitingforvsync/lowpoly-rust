//! Polygon geometry helpers on top of glam.

use glam::{DVec2, DVec3};

/// Newell's method: the area-weighted normal of a (possibly non-planar) polygon, with length
/// equal to twice its area. Counter-clockwise winding (right-hand rule) points it towards the
/// viewer.
pub fn newell_normal(points: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n
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

/// Projects points onto the best-fit plane of `outer` (its Newell normal, or +Z if degenerate),
/// returning a function from 3D points to 2D plane coordinates. Counter-clockwise about the
/// normal stays counter-clockwise in 2D.
pub fn plane_projector(outer: &[DVec3]) -> impl Fn(DVec3) -> DVec2 + use<> {
    let n = newell_normal(outer).normalize_or_zero();
    let (u, v) = plane_basis(if n == DVec3::ZERO { DVec3::Z } else { n });
    move |p| DVec2::new(p.dot(u), p.dot(v))
}

/// Signed area of a 2D polygon, positive when counter-clockwise.
pub fn signed_area_2d(points: &[DVec2]) -> f64 {
    let twice: f64 = points
        .iter()
        .enumerate()
        .map(|(i, p)| p.perp_dot(points[(i + 1) % points.len()]))
        .sum();
    twice / 2.0
}

/// Even-odd point-in-polygon test in 2D.
pub fn point_in_polygon_2d(p: DVec2, polygon: &[DVec2]) -> bool {
    let mut inside = false;
    let mut j = polygon.len().wrapping_sub(1);
    for (i, a) in polygon.iter().enumerate() {
        let b = polygon[j];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: DVec3, b: DVec3) -> bool {
        a.abs_diff_eq(b, 1e-12)
    }

    #[test]
    fn newell_normal_points_up_for_ccw_square_with_length_twice_area() {
        let n = newell_normal(&[
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(2.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ]);
        assert_eq!(n, DVec3::new(0.0, 0.0, 8.0));
    }

    #[test]
    fn newell_normal_flips_with_winding() {
        let n = newell_normal(&[DVec3::ZERO, DVec3::Y, DVec3::X]);
        assert!(n.z < 0.0, "{n}");
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
            assert!(close(u.cross(v), n), "{n}: u×v = {}", u.cross(v));
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
        assert_eq!(signed_area_2d(&square), 4.0);
        square.reverse();
        assert_eq!(signed_area_2d(&square), -4.0);
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
