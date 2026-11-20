//! Triangulation. Ported from the TypeScript app's `core/triangulate.test.ts`.

mod common;

use common::{cube_data, mesh, square_with_hole_data};
use glam::DVec3;
use lowpoly_core::mesh::FaceId;
use lowpoly_core::triangulate::{Triangulator, triangulate_polygon};

fn pts(ps: &[[f64; 3]]) -> Vec<DVec3> {
    ps.iter().map(|&p| DVec3::from(p)).collect()
}

/// Total triangle area, checking every triangle faces along `normal`.
fn checked_area(loops: &[Vec<DVec3>], normal: DVec3) -> f64 {
    let flat: Vec<DVec3> = loops.concat();
    triangulate_polygon(loops)
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| flat[i as usize]);
            let n = (b - a).cross(c - a);
            assert!(
                n.dot(normal) >= 0.0,
                "triangle {t:?} faces away from {normal}"
            );
            n.length() / 2.0
        })
        .sum()
}

fn assert_close(got: f64, want: f64) {
    assert!((got - want).abs() < 1e-9, "got {got}, want {want}");
}

#[test]
fn splits_a_square_into_two_ccw_triangles() {
    let square = pts(&[
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ]);
    assert_eq!(triangulate_polygon(&[&square]).len(), 6);
    assert_close(checked_area(&[square], DVec3::Z), 1.0);
}

#[test]
fn handles_a_concave_l_shape() {
    let l = pts(&[
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 2.0, 0.0],
        [0.0, 2.0, 0.0],
    ]);
    assert_eq!(triangulate_polygon(&[&l]).len(), 12);
    assert_close(checked_area(&[l], DVec3::Z), 3.0);
}

#[test]
fn handles_holes() {
    let d = square_with_hole_data();
    let loops: Vec<Vec<DVec3>> = d.faces[0]
        .loops
        .iter()
        .map(|l| {
            l.iter()
                .map(|c| DVec3::from(d.verts[c.v as usize].p))
                .collect()
        })
        .collect();
    assert_eq!(triangulate_polygon(&loops).len(), 8 * 3);
    assert_close(checked_area(&loops, DVec3::Z), 12.0);
}

#[test]
fn handles_faces_in_any_orientation() {
    // Square in the YZ plane facing -X.
    let square = pts(&[
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
    ]);
    assert_close(checked_area(&[square], DVec3::NEG_X), 1.0);
}

#[test]
fn handles_a_slightly_non_planar_quad() {
    let q = pts(&[
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.05],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.05],
    ]);
    assert_eq!(triangulate_polygon(&[q]).len(), 6);
}

#[test]
fn falls_back_to_a_fan_for_degenerate_faces() {
    let line = pts(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    assert_eq!(triangulate_polygon(&[line]), [0, 1, 2]);
}

#[test]
fn face_corners_carry_uvs_with_triangles_facing_along_the_face_normal() {
    let m = mesh(&cube_data());
    let mut triangulator = Triangulator::new();
    for f in m.faces() {
        let tri = triangulator.triangulate_face(&m, f);
        assert_eq!(tri.corners.len(), 4);
        assert_eq!(tri.indices.len(), 6);
        let n = m.face_normal(f);
        for t in tri.indices.as_chunks::<3>().0 {
            let [a, b, c] =
                [t[0], t[1], t[2]].map(|i| m.position(m.origin(tri.corners[i as usize])));
            assert!((b - a).cross(c - a).dot(n) > 0.0, "face {f}");
        }
    }
}

#[test]
fn flattens_outer_and_hole_corners() {
    let m = mesh(&square_with_hole_data());
    let tri = Triangulator::new().triangulate_face(&m, FaceId::new(0));
    assert_eq!(tri.corners.len(), 8);
    assert_eq!(tri.indices.len(), 24);
}
