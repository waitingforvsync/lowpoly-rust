//! Benchmarks for mesh hot paths. Run with `cargo bench -p lowpoly-core`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use glam::DVec3;
use lowpoly_core::mesh::{Corner, EditableMesh, MaterialId, VertId};
use lowpoly_core::triangulate::Triangulator;

/// An `n` x `n` grid of unit quads on the XY plane.
fn grid(n: u32) -> EditableMesh {
    let mut m = EditableMesh::new();
    let side = n + 1;
    let verts: Vec<VertId> = (0..side * side)
        .map(|i| m.add_vertex(DVec3::new(f64::from(i % side), f64::from(i / side), 0.0)))
        .collect();
    let at = |x: u32, y: u32| Corner::at(verts[(y * side + x) as usize]);
    for y in 0..n {
        for x in 0..n {
            let quad = [at(x, y), at(x + 1, y), at(x + 1, y + 1), at(x, y + 1)];
            m.add_face(&[quad], MaterialId(0))
                .expect("grid quads are valid");
        }
    }
    m
}

fn benches(c: &mut Criterion) {
    c.bench_function("build 100x100 grid", |b| b.iter(|| grid(black_box(100))));

    let mesh = grid(100);
    c.bench_function("clone 100x100 grid and move one vertex", |b| {
        b.iter(|| {
            let mut copy = mesh.clone();
            copy.set_position(VertId::new(5000), DVec3::new(0.5, 0.5, 0.0));
            black_box(copy)
        });
    });

    c.bench_function("split an edge of a 100x100 grid", |b| {
        let e = mesh
            .find_edge(VertId::new(5000), VertId::new(5001))
            .expect("grid edge");
        b.iter(|| {
            let mut copy = mesh.clone();
            black_box(copy.split_edge(e, 0.5).expect("split succeeds"));
            copy
        });
    });

    c.bench_function("triangulate 100x100 grid", |b| {
        let mut triangulator = Triangulator::new();
        b.iter(|| {
            for f in mesh.faces() {
                black_box(triangulator.triangulate_face(&mesh, f));
            }
        });
    });

    let data = mesh.to_data();
    c.bench_function("load 100x100 grid from MeshData", |b| {
        b.iter(|| EditableMesh::from_data(black_box(&data)));
    });
}

criterion_group!(mesh_benches, benches);
criterion_main!(mesh_benches);
