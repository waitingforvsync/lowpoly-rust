//! Test fixtures, mirroring the TypeScript app's `core/testing/fixtures.ts`.

#![allow(dead_code, reason = "each test binary uses a different subset")]
#![allow(unreachable_pub, reason = "shared test helpers")]

use lowpoly_core::mesh::{EditableMesh, VertId};
use lowpoly_core::mesh_data::{CornerData, FaceData, MeshData, VertData};

const QUAD_UVS: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

fn corners(ids: &[u32], uvs: &[[f64; 2]]) -> Vec<CornerData> {
    ids.iter()
        .enumerate()
        .map(|(i, &v)| CornerData {
            v,
            uv: uvs.get(i).copied().unwrap_or([0.0, 0.0]),
        })
        .collect()
}

fn verts(ps: &[[f64; 3]]) -> Vec<VertData> {
    ps.iter()
        .zip(0..)
        .map(|(&p, id)| VertData { id, p })
        .collect()
}

fn face(id: u32, material: u32, loops: Vec<Vec<CornerData>>) -> FaceData {
    FaceData {
        id,
        material,
        loops,
    }
}

/// Unit cube [0,1]^3, outward-facing CCW quads, canonical order.
pub fn cube_data() -> MeshData {
    let quad = |ids: [u32; 4]| vec![corners(&ids, &QUAD_UVS)];
    MeshData {
        verts: verts(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ]),
        faces: vec![
            face(0, 0, quad([0, 3, 2, 1])), // bottom (-Z)
            face(1, 0, quad([4, 5, 6, 7])), // top (+Z)
            face(2, 0, quad([0, 1, 5, 4])), // front (-Y)
            face(3, 0, quad([2, 3, 7, 6])), // back (+Y)
            face(4, 0, quad([0, 4, 7, 3])), // left (-X)
            face(5, 0, quad([1, 2, 6, 5])), // right (+X)
        ],
        wire_edges: vec![],
        soft_edges: vec![],
        next_vert_id: 8,
        next_face_id: 6,
    }
}

/// Single quad on the XY plane (normal +Z), side 1.
pub fn quad_data() -> MeshData {
    MeshData {
        verts: verts(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]),
        faces: vec![face(0, 0, vec![corners(&[0, 1, 2, 3], &QUAD_UVS)])],
        wire_edges: vec![],
        soft_edges: vec![],
        next_vert_id: 4,
        next_face_id: 1,
    }
}

/// Two triangles sharing edge 1-3, together forming the unit square (normal +Z).
pub fn two_triangles_data() -> MeshData {
    MeshData {
        verts: verts(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]),
        faces: vec![
            face(
                0,
                0,
                vec![corners(&[0, 1, 3], &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])],
            ),
            face(
                1,
                1,
                vec![corners(&[1, 2, 3], &[[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])],
            ),
        ],
        wire_edges: vec![],
        soft_edges: vec![],
        next_vert_id: 4,
        next_face_id: 2,
    }
}

/// 4x4 square on XY (normal +Z) with a 2x2 square hole in the middle. UV = xy / 4.
pub fn square_with_hole_data() -> MeshData {
    let ps = [
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [0.0, 4.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 3.0, 0.0],
        [3.0, 3.0, 0.0],
        [3.0, 1.0, 0.0],
    ];
    let uv_loop = |ids: [u32; 4]| -> Vec<CornerData> {
        ids.iter()
            .map(|&v| {
                let p = ps[v as usize];
                CornerData {
                    v,
                    uv: [p[0] / 4.0, p[1] / 4.0],
                }
            })
            .collect()
    };
    MeshData {
        verts: verts(&ps),
        faces: vec![face(
            0,
            0,
            vec![uv_loop([0, 1, 2, 3]), uv_loop([4, 5, 6, 7])],
        )],
        wire_edges: vec![],
        soft_edges: vec![],
        next_vert_id: 8,
        next_face_id: 1,
    }
}

/// Loads fixture data, which is always valid.
pub fn mesh(data: &MeshData) -> EditableMesh {
    EditableMesh::from_data(data).expect("fixture data is valid")
}

/// Shorthand for a vertex handle.
pub fn v(index: u32) -> VertId {
    VertId::new(index)
}

/// Asserts that the mesh passes `validate`, printing the problems if not.
#[track_caller]
pub fn assert_valid(m: &EditableMesh) {
    let errors = m.validate();
    assert!(
        errors.is_empty(),
        "invalid mesh:\n  {}",
        errors.join("\n  ")
    );
}
