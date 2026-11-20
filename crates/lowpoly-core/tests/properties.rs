//! Property tests: random sequences of operators keep the mesh valid, refused operators leave it
//! unchanged, and the result survives a save/load round trip.

mod common;

use common::{cube_data, mesh};
use glam::DVec3;
use lowpoly_core::mesh::{
    Corner, EdgeId, EditableMesh, FaceId, HoleFill, MeshError, RemoveFace, VertId,
};
use proptest::prelude::*;

/// An operator with its targets chosen by index among the live elements, so any value is usable.
#[derive(Clone, Debug)]
enum Op {
    SplitEdge { edge: usize, t: f64 },
    SplitFace { face: usize, a: usize, b: usize },
    SplitFaceThroughCentre { face: usize, a: usize, b: usize },
    JoinFaces { edge: usize },
    AddHole { face: usize, fill: HoleFill },
    FillHole { face: usize },
    RemoveHole { face: usize },
    RemoveFace { face: usize, cleanup: RemoveFace },
}

fn op() -> impl Strategy<Value = Op> {
    let index = || 0_usize..1000;
    let fill = prop_oneof![Just(HoleFill::Open), Just(HoleFill::Filled)];
    let cleanup =
        (any::<bool>(), any::<bool>()).prop_map(|(wire_edges, isolated_verts)| RemoveFace {
            wire_edges,
            isolated_verts,
        });
    prop_oneof![
        3 => (index(), 0.05_f64..0.95).prop_map(|(edge, t)| Op::SplitEdge { edge, t }),
        3 => (index(), index(), index()).prop_map(|(face, a, b)| Op::SplitFace { face, a, b }),
        1 => (index(), index(), index())
            .prop_map(|(face, a, b)| Op::SplitFaceThroughCentre { face, a, b }),
        3 => index().prop_map(|edge| Op::JoinFaces { edge }),
        1 => (index(), fill).prop_map(|(face, fill)| Op::AddHole { face, fill }),
        1 => index().prop_map(|face| Op::FillHole { face }),
        1 => index().prop_map(|face| Op::RemoveHole { face }),
        1 => (index(), cleanup).prop_map(|(face, cleanup)| Op::RemoveFace { face, cleanup }),
    ]
}

fn nth<T>(mut items: impl Iterator<Item = T>, count: usize, pick: usize) -> Option<T> {
    (count > 0).then(|| items.nth(pick % count)).flatten()
}

fn pick_face(m: &EditableMesh, pick: usize) -> Option<FaceId> {
    nth(m.faces(), m.face_count(), pick)
}

fn pick_edge(m: &EditableMesh, pick: usize) -> Option<EdgeId> {
    nth(m.edges(), m.edge_count(), pick)
}

fn outer_verts(m: &EditableMesh, f: FaceId) -> Vec<VertId> {
    m.loop_verts(m.face_outer(f)).collect()
}

fn centroid(m: &EditableMesh, f: FaceId) -> DVec3 {
    let points: Vec<DVec3> = m.loop_positions(m.face_outer(f)).collect();
    points.iter().sum::<DVec3>() / f64::from(u32::try_from(points.len()).expect("small loops"))
}

/// Applies `op`. `Ok(())` when there was nothing to apply it to.
fn apply(m: &mut EditableMesh, op: &Op) -> Result<(), MeshError> {
    match *op {
        Op::SplitEdge { edge, t } => {
            let Some(e) = pick_edge(m, edge) else {
                return Ok(());
            };
            m.split_edge(e, t).map(drop)
        }
        Op::SplitFace { face, a, b } | Op::SplitFaceThroughCentre { face, a, b } => {
            let Some(f) = pick_face(m, face) else {
                return Ok(());
            };
            let verts = outer_verts(m, f);
            let (va, vb) = (verts[a % verts.len()], verts[b % verts.len()]);
            let path = match op {
                Op::SplitFaceThroughCentre { .. } => vec![centroid(m, f)],
                _ => Vec::new(),
            };
            m.split_face(f, va, vb, &path).map(drop)
        }
        Op::JoinFaces { edge } => {
            let Some(e) = pick_edge(m, edge) else {
                return Ok(());
            };
            m.join_faces(e).map(drop)
        }
        Op::AddHole { face, fill } => {
            let Some(f) = pick_face(m, face) else {
                return Ok(());
            };
            // A copy of the outer loop shrunk towards its centroid, in the face's plane.
            let centre = centroid(m, f);
            let corners: Vec<Corner> = outer_verts(m, f)
                .into_iter()
                .map(|v| {
                    let p = centre + (m.position(v) - centre) * 0.3;
                    Corner::at(m.add_vertex(p))
                })
                .collect();
            m.add_hole(f, &corners, fill).map(drop)
        }
        Op::FillHole { face } => {
            let Some(f) = pick_face(m, face) else {
                return Ok(());
            };
            let Some(hole) = m.face_holes(f).next() else {
                return Ok(());
            };
            m.fill_hole(f, hole).map(drop)
        }
        Op::RemoveHole { face } => {
            let Some(f) = pick_face(m, face) else {
                return Ok(());
            };
            let Some(hole) = m.face_holes(f).next() else {
                return Ok(());
            };
            m.remove_hole(f, hole)
        }
        Op::RemoveFace { face, cleanup } => {
            let Some(f) = pick_face(m, face) else {
                return Ok(());
            };
            m.remove_face(f, cleanup)
        }
    }
}

// 256 cases by default; set PROPTEST_CASES to run more.
proptest! {
    #[test]
    fn random_operations_keep_the_mesh_valid(ops in prop::collection::vec(op(), 1..40)) {
        let mut m = mesh(&cube_data());
        for op in &ops {
            let before = m.to_data();
            // AddHole adds its hole vertices before it can be refused, so skip it here.
            if apply(&mut m, op).is_err() && !matches!(op, Op::AddHole { .. }) {
                prop_assert_eq!(m.to_data(), before, "refused {:?} changed the mesh", op);
            }
            let errors = m.validate();
            prop_assert!(errors.is_empty(), "after {:?}:\n{}", op, errors.join("\n"));
        }

        let reloaded = EditableMesh::from_data(&m.to_data()).expect("saved meshes load");
        prop_assert!(reloaded.validate().is_empty());
        prop_assert_eq!(reloaded.vert_count(), m.vert_count());
        prop_assert_eq!(reloaded.edge_count(), m.edge_count());
        prop_assert_eq!(reloaded.face_count(), m.face_count());
        let compact = reloaded.to_data();
        prop_assert_eq!(EditableMesh::from_data(&compact).expect("reloads").to_data(), compact);
    }
}
