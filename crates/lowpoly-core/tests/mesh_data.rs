//! Conversion between `MeshData` and `EditableMesh`. Ported from the TypeScript app's
//! `core/halfedge/convert.test.ts`.

mod common;

use common::{assert_valid, cube_data, mesh, square_with_hole_data, two_triangles_data, v};
use glam::DVec2;
use lowpoly_core::mesh::{FaceId, MeshError};
use lowpoly_core::mesh_data::{CornerData, DataRef, FaceData, LoadMeshError, MeshData, VertData};

#[test]
fn round_trips_fixtures_exactly() {
    for (name, data) in [
        ("cube", cube_data()),
        ("two triangles", two_triangles_data()),
        ("square with hole", square_with_hole_data()),
    ] {
        let m = mesh(&data);
        assert!(m.validate().is_empty(), "{name}: {:?}", m.validate());
        assert_eq!(m.to_data(), data, "{name}");
    }
}

#[test]
fn builds_the_expected_cube_topology() {
    let m = mesh(&cube_data());
    assert_eq!(m.vert_count(), 8);
    assert_eq!(m.edge_count(), 12);
    assert_eq!(m.face_count(), 6);
    assert!(m.edges().all(|e| m.edge_faces(e).count() == 2));
    assert!(m.verts().all(|x| m.valence(x) == 3));
    // Euler characteristic of a closed genus-0 surface.
    assert_eq!(m.vert_count() + m.face_count() - m.edge_count(), 2);
}

#[test]
fn builds_hole_loops() {
    let m = mesh(&square_with_hole_data());
    let holes: Vec<_> = m.face_holes(FaceId::new(0)).collect();
    assert_eq!(holes.len(), 1);
    assert!(m.is_hole(holes[0]));
    assert_eq!(
        m.loop_verts(holes[0]).collect::<Vec<_>>(),
        [v(4), v(5), v(6), v(7)]
    );
    // Hole edges are boundary edges: the face on one side, open on the other.
    for h in m.loop_half_edges(holes[0]) {
        assert!(m.is_boundary(h.edge()));
        assert!(m.face_of(h.twin()).is_none());
    }
}

#[test]
fn preserves_per_corner_uvs() {
    let m = mesh(&two_triangles_data());
    let h = m.find_half_edge(v(1), v(3)).unwrap();
    assert_eq!(m.face_of(h), Some(FaceId::new(0)));
    assert_eq!(m.uv(h), DVec2::new(1.0, 0.0));
    let t = h.twin();
    assert_eq!(m.face_of(t), Some(FaceId::new(1)));
    assert_eq!(m.uv(t), DVec2::new(0.0, 1.0));
}

#[test]
fn round_trips_wire_edges_and_isolated_vertices() {
    let mut data = cube_data();
    data.verts.extend([
        VertData {
            id: 8,
            p: [5.0, 5.0, 5.0],
        },
        VertData {
            id: 9,
            p: [6.0, 5.0, 5.0],
        },
        VertData {
            id: 10,
            p: [9.0, 9.0, 9.0],
        },
    ]);
    data.wire_edges.extend([[0, 8], [8, 9]]);
    data.next_vert_id = 11;
    let m = mesh(&data);
    assert_valid(&m);
    assert!(m.is_wire(m.find_edge(v(0), v(8)).unwrap()));
    assert_eq!(m.to_data(), data);
}

#[test]
fn round_trips_soft_edges() {
    let mut data = cube_data();
    data.soft_edges.push([0, 1]);
    assert_eq!(mesh(&data).to_data(), data);
}

#[test]
fn remaps_sparse_file_ids_to_dense_handles() {
    // Replaces the TS test of stale nextVertId/nextFaceId counters, which handles don't have.
    let mut data = two_triangles_data();
    for vert in &mut data.verts {
        vert.id = vert.id * 10 + 3;
    }
    for corner in data
        .faces
        .iter_mut()
        .flat_map(|f| f.loops.iter_mut().flatten())
    {
        corner.v = corner.v * 10 + 3;
    }
    data.faces[1].id = 40;
    let m = mesh(&data);
    assert_valid(&m);
    assert_eq!(m.to_data(), two_triangles_data());
}

#[test]
fn json_matches_the_typescript_format() {
    let json = serde_json::to_value(two_triangles_data()).unwrap();
    assert!(json.get("wireEdges").is_some());
    assert!(json.get("nextVertId").is_some());
    assert_eq!(
        json["faces"][0]["loops"][0][1],
        serde_json::json!({ "v": 1, "uv": [1.0, 0.0] })
    );

    // Older files have no softEdges.
    let mut legacy = json;
    legacy.as_object_mut().unwrap().remove("softEdges");
    let parsed: MeshData = serde_json::from_value(legacy).unwrap();
    assert_eq!(parsed, two_triangles_data());
}

fn rejects(mutate: impl FnOnce(&mut MeshData)) -> LoadMeshError {
    let mut data = cube_data();
    mutate(&mut data);
    lowpoly_core::mesh::EditableMesh::from_data(&data).expect_err("data is invalid")
}

fn corner(v: u32) -> CornerData {
    CornerData { v, uv: [0.0, 0.0] }
}

#[test]
fn rejects_a_non_manifold_edge() {
    let err = rejects(|d| {
        d.verts.push(VertData {
            id: 8,
            p: [0.5, -1.0, 0.5],
        });
        // Edge 0-1 already has the bottom (1->0) and front (0->1) faces.
        d.faces.push(FaceData {
            id: 6,
            material: 0,
            loops: vec![vec![corner(0), corner(1), corner(8)]],
        });
    });
    assert!(
        matches!(
            err,
            LoadMeshError::InvalidFace {
                id: 6,
                source: MeshError::HalfEdgeInUse { .. }
            }
        ),
        "{err}"
    );
}

#[test]
fn rejects_inconsistent_orientation() {
    let err = rejects(|d| d.faces[1].loops[0].reverse());
    assert!(
        matches!(
            err,
            LoadMeshError::InvalidFace {
                source: MeshError::HalfEdgeInUse { .. },
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn rejects_a_repeated_vertex() {
    let err = rejects(|d| d.faces[0].loops[0][1].v = 0);
    assert!(
        matches!(
            err,
            LoadMeshError::InvalidFace {
                source: MeshError::RepeatedVertex(_),
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn rejects_a_loop_with_fewer_than_3_corners() {
    let err = rejects(|d| d.faces[0].loops[0].truncate(2));
    assert!(
        matches!(
            err,
            LoadMeshError::InvalidFace {
                source: MeshError::TooFewCorners(2),
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn rejects_an_unknown_vertex() {
    assert_eq!(
        rejects(|d| d.faces[0].loops[0][0].v = 99),
        LoadMeshError::UnknownVertex {
            vertex: 99,
            referrer: DataRef::Face(0)
        }
    );
}

#[test]
fn rejects_a_duplicate_vertex_id() {
    let err = rejects(|d| {
        d.verts.push(VertData {
            id: 0,
            p: [0.0, 0.0, 0.0],
        });
    });
    assert_eq!(err, LoadMeshError::DuplicateVertexId(0));
}

#[test]
fn rejects_a_duplicate_face_id() {
    assert_eq!(
        rejects(|d| d.faces[1].id = 0),
        LoadMeshError::DuplicateFaceId(0)
    );
}

#[test]
fn rejects_a_wire_edge_duplicating_a_face_edge() {
    let err = rejects(|d| d.wire_edges.push([0, 1]));
    assert!(
        matches!(
            err,
            LoadMeshError::InvalidWireEdge {
                source: MeshError::EdgeExists { .. },
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn rejects_an_unknown_soft_edge() {
    assert_eq!(
        rejects(|d| d.soft_edges.push([0, 6])),
        LoadMeshError::UnknownSoftEdge(0, 6)
    );
}

#[test]
fn rejects_an_unknown_vertex_in_a_wire_edge() {
    assert_eq!(
        rejects(|d| d.wire_edges.push([0, 42])),
        LoadMeshError::UnknownVertex {
            vertex: 42,
            referrer: DataRef::WireEdge(0, 42)
        }
    );
}

#[test]
fn load_errors_name_the_element_without_repeating_the_cause() {
    use std::error::Error as _;
    let err = rejects(|d| d.faces[0].loops[0][1].v = 0);
    assert_eq!(err.to_string(), "invalid face 0");
    let cause = err.source().expect("the mesh error is the source");
    assert_eq!(
        cause.to_string(),
        "vertex 0 appears more than once in the face"
    );
}

#[test]
fn saving_a_mesh_with_dead_elements_compacts_on_reload() {
    let mut m = mesh(&cube_data());
    m.remove_face(
        FaceId::new(1),
        lowpoly_core::mesh::RemoveFace {
            wire_edges: true,
            isolated_verts: true,
        },
    )
    .unwrap();
    m.split_edge(m.find_edge(v(0), v(1)).unwrap(), 0.5).unwrap();
    let sparse = m.to_data();
    assert_eq!(sparse.next_face_id, 6, "slot counts include dead faces");

    let reloaded = mesh(&sparse);
    assert_valid(&reloaded);
    assert_eq!(reloaded.vert_count(), m.vert_count());
    assert_eq!(reloaded.edge_count(), m.edge_count());
    assert_eq!(reloaded.face_count(), m.face_count());
    let compact = reloaded.to_data();
    assert!(
        compact.faces.iter().zip(0..).all(|(f, i)| f.id == i),
        "dense face ids"
    );
    assert_eq!(
        mesh(&compact).to_data(),
        compact,
        "compaction is idempotent"
    );
}

/// Two triangles exactly as the TypeScript app's `JSON.stringify(toMeshData(..))` writes them:
/// integral numbers have no fractional part, and keys follow `toMeshData`'s order.
const TS_TWO_TRIANGLES: &str = r#"{"verts":[{"id":0,"p":[0,0,0]},{"id":1,"p":[1,0,0]},{"id":2,"p":[1,1,0]},{"id":3,"p":[0,1,0]}],"faces":[{"id":0,"material":0,"loops":[[{"v":0,"uv":[0,0]},{"v":1,"uv":[1,0]},{"v":3,"uv":[0,1]}]]},{"id":1,"material":1,"loops":[[{"v":1,"uv":[1,0]},{"v":2,"uv":[1,1]},{"v":3,"uv":[0,1]}]]}],"wireEdges":[],"softEdges":[],"nextVertId":4,"nextFaceId":2}"#;

#[test]
fn reads_mesh_json_written_by_the_typescript_app() {
    let data: MeshData = serde_json::from_str(TS_TWO_TRIANGLES).unwrap();
    assert_eq!(data, two_triangles_data());
    let reparsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&mesh(&data).to_data()).unwrap()).unwrap();
    let original: serde_json::Value = serde_json::from_str(TS_TWO_TRIANGLES).unwrap();
    assert_eq!(
        reparsed.as_object().unwrap().keys().collect::<Vec<_>>(),
        original.as_object().unwrap().keys().collect::<Vec<_>>(),
        "same top-level keys"
    );
}
