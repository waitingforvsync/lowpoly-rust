//! Topological operators. Ported from the TypeScript app's `core/halfedge/euler.test.ts`.

mod common;

use common::{
    assert_valid, cube_data, mesh, quad_data, square_with_hole_data, two_triangles_data, v,
};
use glam::{DVec2, DVec3};
use lowpoly_core::mesh::{Corner, EditableMesh, FaceId, HoleFill, MaterialId, MeshError, VertId};

const M0: MaterialId = MaterialId(0);

fn ids(it: impl Iterator<Item = VertId>) -> Vec<u32> {
    it.map(VertId::index).collect()
}

fn outer_ids(m: &EditableMesh, f: FaceId) -> Vec<u32> {
    ids(m.loop_verts(m.face_outer(f)))
}

/// A 6x2 rectangle with a hole in its right half, as in the TS tests. Returns the mesh, the
/// face, and the outer vertices.
fn rectangle_with_hole() -> (EditableMesh, FaceId, Vec<VertId>) {
    let mut m = EditableMesh::new();
    let mut p = |x: f64, y: f64| m.add_vertex(DVec3::new(x, y, 0.0));
    let outer = vec![
        p(0.0, 0.0),
        p(3.0, 0.0),
        p(6.0, 0.0),
        p(6.0, 2.0),
        p(3.0, 2.0),
        p(0.0, 2.0),
    ];
    let hole = [p(4.0, 0.5), p(4.0, 1.5), p(5.0, 1.5), p(5.0, 0.5)]; // CW
    let loops: Vec<Vec<Corner>> = vec![
        outer.iter().map(|&x| Corner::at(x)).collect(),
        hole.iter().map(|&x| Corner::at(x)).collect(),
    ];
    let f = m.add_face(&loops, M0).expect("the rectangle is valid");
    (m, f, outer)
}

// ---- split_edge ----

#[test]
fn split_edge_inserts_a_vertex_into_both_adjacent_faces_with_interpolated_uvs() {
    let mut m = mesh(&cube_data());
    let (a, b) = (v(0), v(1));
    let new = m.split_edge(m.find_edge(a, b).unwrap(), 0.25).unwrap();
    assert_valid(&m);
    assert_eq!(new, v(8));

    // The edge's stored direction decides what t means; check the position is a quarter of the
    // way from one endpoint.
    let p = m.position(new);
    assert!(
        (p.x - 0.25).abs() < 1e-12 || (p.x - 0.75).abs() < 1e-12,
        "{p}"
    );
    assert_eq!((p.y, p.z), (0.0, 0.0));

    assert_eq!(m.edge_count(), 13);
    assert!(m.find_edge(a, b).is_none());
    assert_eq!(outer_ids(&m, FaceId::new(0)).len(), 5); // bottom
    assert_eq!(outer_ids(&m, FaceId::new(2)).len(), 5); // front

    // Front face: 0 (0,0) -> 1 (1,0). The new corner's UV lies on that UV edge at the same t.
    let h = m.find_half_edge(new, b).unwrap();
    assert_eq!(m.face_of(h), Some(FaceId::new(2)));
    assert!(
        (m.uv(h).x - p.x).abs() < 1e-12,
        "uv {} vs x {}",
        m.uv(h),
        p.x
    );
    assert_eq!(m.uv(h).y, 0.0);
}

#[test]
fn split_edge_measures_t_from_the_primary_half_edge_origin() {
    let mut m = mesh(&quad_data());
    let e = m.find_edge(v(0), v(1)).unwrap();
    let origin = m.edge_verts(e)[0];
    let new = m.split_edge(e, 0.25).unwrap();
    assert!((m.position(new).distance(m.position(origin)) - 0.25).abs() < 1e-12);
}

#[test]
fn split_edge_splits_boundary_and_wire_edges() {
    let mut m = mesh(&quad_data());
    m.split_edge(m.find_edge(v(0), v(1)).unwrap(), 0.5).unwrap();
    assert_valid(&m);
    assert_eq!(outer_ids(&m, FaceId::new(0)).len(), 5);

    let mut w = EditableMesh::new();
    let a = w.add_vertex(DVec3::ZERO);
    let b = w.add_vertex(DVec3::new(2.0, 0.0, 0.0));
    let e = w.add_edge(a, b).unwrap();
    let mid = w.split_edge(e, 0.5).unwrap();
    assert_valid(&w);
    assert_eq!(w.position(mid), DVec3::new(1.0, 0.0, 0.0));
    assert!(w.is_wire(w.find_edge(a, mid).unwrap()));
    assert!(w.is_wire(w.find_edge(mid, b).unwrap()));
    assert!(w.find_edge(a, b).is_none());
}

#[test]
fn split_edge_rejects_a_dead_edge() {
    // Replaces the TS test of explicit vertex ids, which handles don't have.
    let mut m = mesh(&quad_data());
    let e = m.find_edge(v(0), v(1)).unwrap();
    m.split_edge(e, 0.5).unwrap();
    assert_eq!(m.split_edge(e, 0.5), Err(MeshError::NoSuchEdge(e)));
}

#[test]
fn split_edge_keeps_shading_on_both_halves() {
    use lowpoly_core::mesh::Shading;
    let mut m = mesh(&quad_data());
    let e = m.find_edge(v(0), v(1)).unwrap();
    m.set_shading(e, Shading::Soft);
    let mid = m.split_edge(e, 0.5).unwrap();
    assert_eq!(m.shading(m.find_edge(v(0), mid).unwrap()), Shading::Soft);
    assert_eq!(m.shading(m.find_edge(mid, v(1)).unwrap()), Shading::Soft);
}

// ---- split_face ----

#[test]
fn split_face_splits_a_quad_into_two_triangles() {
    let mut m = mesh(&quad_data());
    let (fa, fb) = m.split_face(FaceId::new(0), v(0), v(2), &[]).unwrap();
    assert_valid(&m);
    assert_eq!(fa, FaceId::new(0));
    assert_eq!(fb, FaceId::new(1));
    assert_eq!(outer_ids(&m, fa), [0, 1, 2]);
    assert_eq!(outer_ids(&m, fb), [2, 3, 0]);
    assert_eq!(m.edge_faces(m.find_edge(v(0), v(2)).unwrap()).count(), 2);
    // UVs are carried across.
    assert_eq!(
        m.uv(m.find_half_edge(v(2), v(3)).unwrap()),
        DVec2::new(1.0, 1.0)
    );
}

#[test]
fn split_face_rejects_adjacent_or_foreign_vertices() {
    let mut m = mesh(&cube_data());
    let top = FaceId::new(1);
    assert_eq!(
        m.split_face(top, v(4), v(5), &[]),
        Err(MeshError::AdjacentVertices)
    );
    assert_eq!(
        m.split_face(top, v(4), v(0), &[]),
        Err(MeshError::NotOnOuterLoop)
    );
    assert_eq!(m.to_data(), cube_data());
}

#[test]
fn split_face_sends_holes_to_the_side_that_contains_them() {
    let (mut m, f, outer) = rectangle_with_hole();
    assert_valid(&m);
    let (left, right) = m.split_face(f, outer[4], outer[1], &[]).unwrap();
    assert_valid(&m);
    // Left is the 3-wide side running from (3,2) through (0,*) to (3,0).
    assert_eq!(outer_ids(&m, left), [4, 5, 0, 1]);
    assert_eq!(m.face_holes(left).count(), 0);
    assert_eq!(m.face_holes(right).count(), 1);
}

#[test]
fn split_face_refuses_to_cut_through_a_hole() {
    let mut m = mesh(&square_with_hole_data());
    assert_eq!(
        m.split_face(FaceId::new(0), v(0), v(2), &[]),
        Err(MeshError::CrossesHole)
    );
    assert_eq!(m.to_data(), square_with_hole_data());
}

#[test]
fn split_face_along_a_path_continues_the_texture() {
    // Rust-only: the path variant was exercised only indirectly (draw tools) in TS.
    let mut m = mesh(&quad_data());
    let path = [DVec3::new(0.5, 0.25, 0.0), DVec3::new(0.5, 0.75, 0.0)];
    let (fa, fb) = m.split_face(FaceId::new(0), v(0), v(3), &path).unwrap();
    assert_valid(&m);
    assert_eq!(m.face_count(), 2);
    assert_eq!(m.vert_count(), 6);
    for f in [fa, fb] {
        for h in m.face_half_edges(f) {
            let p = m.position(m.origin(h));
            assert_eq!(m.uv(h), DVec2::new(p.x, p.y), "quad UVs equal xy");
        }
    }
}

#[test]
fn split_face_rejects_a_path_out_of_the_plane() {
    let mut m = mesh(&quad_data());
    let path = [DVec3::new(0.5, 0.5, 0.3)];
    assert_eq!(
        m.split_face(FaceId::new(0), v(0), v(2), &path),
        Err(MeshError::PathNotPlanar)
    );
}

// ---- join_faces ----

#[test]
fn join_faces_merges_two_triangles_into_a_quad() {
    let mut m = mesh(&two_triangles_data());
    let f = m.join_faces(m.find_edge(v(1), v(3)).unwrap()).unwrap();
    assert_valid(&m);
    assert_eq!(m.face_count(), 1);
    assert_eq!(m.edge_count(), 4);
    assert_eq!(outer_ids(&m, f).len(), 4);
    // Corner UVs come from the faces they belonged to.
    let uv_of = |id: u32| {
        let h = m
            .loop_half_edges(m.face_outer(f))
            .find(|&h| m.origin(h) == v(id))
            .unwrap();
        m.uv(h)
    };
    assert_eq!(uv_of(0), DVec2::new(0.0, 0.0));
    assert_eq!(uv_of(2), DVec2::new(1.0, 1.0));
}

#[test]
fn join_faces_keeps_the_id_and_material_of_the_primary_side() {
    let mut m = mesh(&two_triangles_data());
    let e = m.find_edge(v(1), v(3)).unwrap();
    let keep = m.face_of(m.edge_half_edge(e)).unwrap();
    let material = m.material(keep);
    let f = m.join_faces(e).unwrap();
    assert_eq!(f, keep);
    assert_eq!(m.material(f), material);
}

#[test]
fn join_faces_undoes_split_face() {
    let mut m = mesh(&quad_data());
    m.split_face(FaceId::new(0), v(1), v(3), &[]).unwrap();
    let f = m.join_faces(m.find_edge(v(1), v(3)).unwrap()).unwrap();
    assert_valid(&m);
    let mut ring = outer_ids(&m, f);
    let start = ring.iter().position(|&x| x == 0).unwrap();
    ring.rotate_left(start);
    assert_eq!(ring, [0, 1, 2, 3], "same cyclic order as the original quad");
}

#[test]
fn join_faces_carries_holes_from_both_faces() {
    let (mut m, f, outer) = rectangle_with_hole();
    m.split_face(f, outer[4], outer[1], &[]).unwrap();
    let joined = m
        .join_faces(m.find_edge(outer[1], outer[4]).unwrap())
        .unwrap();
    assert_valid(&m);
    assert_eq!(m.face_holes(joined).count(), 1);
}

#[test]
fn join_faces_rejects_boundary_edges_and_faces_sharing_several_edges() {
    let mut m = mesh(&quad_data());
    assert_eq!(
        m.join_faces(m.find_edge(v(0), v(1)).unwrap()),
        Err(MeshError::NotInteriorEdge)
    );

    // A quad fanned into 4 triangles round a centre vertex: after merging three of them, the
    // result shares two edges (both meeting at the centre) with the last triangle.
    let mut fan = EditableMesh::new();
    let c: Vec<VertId> = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        .iter()
        .map(|&[x, y]| fan.add_vertex(DVec3::new(x, y, 0.0)))
        .collect();
    let mid = fan.add_vertex(DVec3::new(0.5, 0.5, 0.0));
    for i in 0..4 {
        fan.add_face(
            &[[
                Corner::at(c[i]),
                Corner::at(c[(i + 1) % 4]),
                Corner::at(mid),
            ]],
            M0,
        )
        .unwrap();
    }
    fan.join_faces(fan.find_edge(c[1], mid).unwrap()).unwrap();
    fan.join_faces(fan.find_edge(c[2], mid).unwrap()).unwrap();
    assert_valid(&fan);
    assert_eq!(
        fan.join_faces(fan.find_edge(c[3], mid).unwrap()),
        Err(MeshError::SharedEdges(2))
    );
    assert_valid(&fan);
}

#[test]
fn join_faces_refuses_faces_that_are_not_coplanar() {
    let mut m = mesh(&cube_data());
    assert_eq!(
        m.join_faces(m.find_edge(v(4), v(5)).unwrap()),
        Err(MeshError::NotCoplanar)
    );
    assert_eq!(m.to_data(), cube_data());
}

// ---- holes ----

#[derive(Clone, Copy)]
enum Winding {
    Clockwise,
    CounterClockwise,
}

fn inner_square(m: &mut EditableMesh, winding: Winding) -> Vec<Corner> {
    let mut pts = [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]];
    if matches!(winding, Winding::Clockwise) {
        pts.reverse();
    }
    pts.iter()
        .map(|&[x, y]| Corner::at(m.add_vertex(DVec3::new(x, y, 0.0))))
        .collect()
}

#[test]
fn add_hole_cuts_a_hole_and_can_fill_it() {
    let mut m = mesh(&quad_data());
    // Counter-clockwise, like the face, so add_hole must flip it.
    let inner = inner_square(&mut m, Winding::CounterClockwise);
    let added = m
        .add_hole(FaceId::new(0), &inner, HoleFill::Filled)
        .unwrap();
    assert_valid(&m);
    assert_eq!(
        m.face_holes(FaceId::new(0)).collect::<Vec<_>>(),
        [added.hole]
    );
    let fill = added.fill.expect("a fill face");
    assert_eq!(m.face_normal(fill), DVec3::Z);
    // The hole's edges are now interior edges between the outer face and the fill face.
    for h in m.loop_half_edges(added.hole) {
        assert_eq!(m.face_of(m.twin(h)), Some(fill));
    }
}

#[test]
fn add_hole_leaves_the_mesh_unchanged_when_refused() {
    // The TS test forced a failure part-way through the rebuild with a clashing face id. Ids are
    // allocated, so this checks a refusal after the hole vertices were created instead; partial
    // rebuilds are covered by `transaction_rolls_back_every_change_on_error`.
    let mut m = mesh(&quad_data());
    let mut inner = inner_square(&mut m, Winding::Clockwise);
    inner[1].v = m.add_vertex(DVec3::new(0.25, 0.75, 0.5)); // lifts one corner off the plane
    let before = m.to_data();
    assert_eq!(
        m.add_hole(FaceId::new(0), &inner, HoleFill::Filled),
        Err(MeshError::NotCoplanar)
    );
    assert_valid(&m);
    assert_eq!(m.to_data(), before);
}

#[test]
fn add_hole_rejects_vertices_already_in_the_face() {
    let mut m = mesh(&quad_data());
    let a = m.add_vertex(DVec3::new(0.5, 0.5, 0.0));
    let b = m.add_vertex(DVec3::new(0.6, 0.5, 0.0));
    let corners = [Corner::at(v(0)), Corner::at(a), Corner::at(b)];
    assert_eq!(
        m.add_hole(FaceId::new(0), &corners, HoleFill::Open),
        Err(MeshError::HoleVertexInFace)
    );
}

#[test]
fn fill_hole_creates_a_face_with_matching_uvs() {
    let mut m = mesh(&square_with_hole_data());
    let face = FaceId::new(0);
    let hole = m.face_holes(face).next().unwrap();
    let fill = m.fill_hole(face, hole).unwrap();
    assert_valid(&m);
    assert_eq!(m.face_normal(fill), DVec3::Z);
    for h in m.loop_half_edges(m.face_outer(fill)) {
        let p = m.position(m.origin(h));
        assert_eq!(m.uv(h), DVec2::new(p.x / 4.0, p.y / 4.0));
    }
}

#[test]
fn remove_hole_closes_an_open_hole_and_deletes_its_edges_and_vertices() {
    let mut m = mesh(&square_with_hole_data());
    let face = FaceId::new(0);
    let hole = m.face_holes(face).next().unwrap();
    m.remove_hole(face, hole).unwrap();
    assert_valid(&m);
    assert_eq!(m.face_holes(face).count(), 0);
    assert_eq!(m.vert_count(), 4);
    assert_eq!(m.edge_count(), 4);
}

#[test]
fn remove_hole_refuses_a_filled_hole() {
    let mut m = mesh(&square_with_hole_data());
    let face = FaceId::new(0);
    let hole = m.face_holes(face).next().unwrap();
    m.fill_hole(face, hole).unwrap();
    assert_eq!(m.remove_hole(face, hole), Err(MeshError::HoleFilled));
    assert_valid(&m);
}

#[test]
fn hole_operations_reject_a_loop_that_is_not_a_hole() {
    let mut m = mesh(&square_with_hole_data());
    let face = FaceId::new(0);
    let outer = m.face_outer(face);
    assert_eq!(m.fill_hole(face, outer), Err(MeshError::NotAHole(outer)));
}
