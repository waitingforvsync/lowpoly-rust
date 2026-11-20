//! Editable mesh primitives. Ported from the TypeScript app's `core/halfedge/mesh.test.ts`.

mod common;

use common::{assert_valid, cube_data, mesh, quad_data, v};
use glam::DVec3;
use lowpoly_core::mesh::{Corner, EditableMesh, FaceId, MaterialId, MeshError, RemoveFace, VertId};

const M0: MaterialId = MaterialId(0);

fn corners(vs: &[VertId]) -> Vec<Corner> {
    vs.iter().map(|&x| Corner::at(x)).collect()
}

#[test]
fn builds_a_face_from_scratch_creating_its_edges() {
    let mut m = EditableMesh::new();
    let a = m.add_vertex(DVec3::new(0.0, 0.0, 0.0));
    let b = m.add_vertex(DVec3::new(1.0, 0.0, 0.0));
    let c = m.add_vertex(DVec3::new(0.0, 1.0, 0.0));
    let f = m.add_face(&[corners(&[a, b, c])], M0).unwrap();
    assert_valid(&m);
    assert_eq!(m.edge_count(), 3);
    assert_eq!(m.loop_verts(m.face_outer(f)).collect::<Vec<_>>(), [a, b, c]);
    assert_eq!(m.face_normal(f), DVec3::Z);
    assert!(m.edges().all(|e| m.is_boundary(e)));
}

#[test]
fn assigns_increasing_ids() {
    // The TS test also checked explicit ids; handles are always allocated in order here.
    let mut m = EditableMesh::new();
    assert_eq!(m.add_vertex(DVec3::ZERO), v(0));
    assert_eq!(m.add_vertex(DVec3::ZERO), v(1));
}

#[test]
fn reuses_existing_edges_when_an_adjacent_face_is_added() {
    let mut m = mesh(&quad_data());
    let v4 = m.add_vertex(DVec3::new(2.0, 0.0, 0.0));
    let v5 = m.add_vertex(DVec3::new(2.0, 1.0, 0.0));
    m.add_face(&[corners(&[v(1), v4, v5, v(2)])], M0).unwrap();
    assert_valid(&m);
    assert_eq!(m.edge_count(), 7);
    let shared = m.find_edge(v(1), v(2)).unwrap();
    assert_eq!(m.edge_faces(shared).count(), 2);
}

#[test]
fn leaves_the_mesh_unchanged_when_add_face_fails() {
    let mut m = mesh(&quad_data());
    let v4 = m.add_vertex(DVec3::new(5.0, 5.0, 0.0));
    let before = m.to_data();
    // 0->1 is already used by the quad, so this face is rejected.
    let result = m.add_face(&[corners(&[v(3), v4, v(0), v(1)])], M0);
    assert!(
        matches!(result, Err(MeshError::HalfEdgeInUse { .. })),
        "{result:?}"
    );
    assert_eq!(m.to_data(), before);
    assert_valid(&m);
}

#[test]
fn remove_face_keeps_edges_by_default_as_boundary_or_wire_edges() {
    let mut m = mesh(&cube_data());
    m.remove_face(FaceId::new(1), RemoveFace::default())
        .unwrap();
    assert_valid(&m);
    assert_eq!(m.face_count(), 5);
    assert_eq!(m.edge_count(), 12);
    assert!(m.is_boundary(m.find_edge(v(4), v(5)).unwrap()));
}

#[test]
fn remove_face_can_clean_up_wire_edges_and_isolated_vertices() {
    let mut m = mesh(&quad_data());
    m.remove_face(
        FaceId::new(0),
        RemoveFace {
            wire_edges: true,
            isolated_verts: true,
        },
    )
    .unwrap();
    assert_valid(&m);
    assert_eq!(m.edge_count(), 0);
    assert_eq!(m.vert_count(), 0);
}

#[test]
fn remove_face_cleanup_leaves_shared_edges_and_vertices_alone() {
    let mut m = mesh(&cube_data());
    m.remove_face(
        FaceId::new(1),
        RemoveFace {
            wire_edges: true,
            isolated_verts: true,
        },
    )
    .unwrap();
    assert_valid(&m);
    assert_eq!(m.edge_count(), 12);
    assert_eq!(m.vert_count(), 8);
}

#[test]
fn guards_edge_and_vertex_removal() {
    let mut m = mesh(&quad_data());
    let e = m.find_edge(v(0), v(1)).unwrap();
    assert_eq!(
        m.remove_edge(e),
        Err(MeshError::EdgeBordersFace(v(0), v(1)))
    );
    assert_eq!(
        m.remove_vertex(v(0)),
        Err(MeshError::VertexHasEdges {
            vert: v(0),
            count: 2
        })
    );
    assert_eq!(
        m.add_edge(v(0), v(1)),
        Err(MeshError::EdgeExists(v(0), v(1)))
    );
    assert_eq!(m.add_edge(v(0), v(0)), Err(MeshError::SelfEdge(v(0))));
}

#[test]
fn rejects_vertices_that_are_not_live() {
    // The TS test passed a vertex object from another mesh. Handles are plain indices, so the
    // equivalent mistake is a handle that doesn't exist (or no longer exists) in this mesh.
    let mut m = mesh(&quad_data());
    let result = m.add_face(&[corners(&[v(9), v(1), v(2)])], M0);
    assert_eq!(result, Err(MeshError::NoSuchVertex(v(9))));
}

#[test]
fn wire_edges_add_and_remove() {
    let mut m = EditableMesh::new();
    let a = m.add_vertex(DVec3::ZERO);
    let b = m.add_vertex(DVec3::X);
    let e = m.add_edge(a, b).unwrap();
    assert!(m.is_wire(e));
    assert_valid(&m);
    m.remove_edge(e).unwrap();
    assert_eq!(m.edge_count(), 0);
    assert_eq!(m.valence(a), 0);
    assert_valid(&m);
}

// Rust-only: the transaction and structural-sharing guarantees the design relies on.

#[test]
fn transaction_rolls_back_every_change_on_error() {
    let mut m = mesh(&cube_data());
    let before = m.to_data();
    let result: Result<(), MeshError> = m.transaction(|m| {
        m.remove_face(
            FaceId::new(0),
            RemoveFace {
                wire_edges: true,
                isolated_verts: true,
            },
        )?;
        m.add_vertex(DVec3::ONE);
        m.set_position(v(3), DVec3::splat(9.0));
        Err(MeshError::NoLoops)
    });
    assert_eq!(result, Err(MeshError::NoLoops));
    assert_eq!(m.to_data(), before);
    assert_valid(&m);
}

#[test]
fn clones_are_independent() {
    let m = mesh(&cube_data());
    let mut copy = m.clone();
    copy.set_position(v(0), DVec3::new(42.0, 0.0, 0.0));
    copy.remove_face(FaceId::new(0), RemoveFace::default())
        .unwrap();
    assert_eq!(m.position(v(0)), DVec3::ZERO);
    assert_eq!(m.to_data(), cube_data());
    assert_eq!(copy.face_count(), 5);
}

#[test]
fn removed_elements_keep_their_slots_and_handles_are_not_reused() {
    let mut m = mesh(&quad_data());
    m.remove_face(FaceId::new(0), RemoveFace::default())
        .unwrap();
    let f = m
        .add_face(&[corners(&[v(0), v(1), v(2), v(3)])], M0)
        .unwrap();
    assert_eq!(f, FaceId::new(1), "a new face gets a new handle");
    assert!(!m.contains_face(FaceId::new(0)));
    assert_valid(&m);
}
