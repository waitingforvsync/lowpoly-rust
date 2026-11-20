# Porting notes

How the TypeScript app (`../lowpoly`) maps to this Rust port, milestone by milestone. The TS
code is the behavioural spec; the Rust architecture differs on purpose (see [PLAN.md](PLAN.md)).

## R1 — Mesh core

### API mapping

| TypeScript | Rust |
|---|---|
| `math/vec.ts` (tuple vectors) | `glam::DVec2` / `DVec3`; `lowpoly_core::math` for `newell_normal`, `plane_basis`, `signed_area_2d`, `point_in_polygon_2d`, plus `plane_projector` (was a private helper in `euler.ts`) |
| `planarity.ts` | `lowpoly_core::planarity` (`planar_deviation`, `planar_tolerance`, `is_planar`) |
| `uv/project.ts`, `uv/continue.ts` (`planarAxes`, `planarUV`, `faceUVMap`, `faceUVFunction`) | `lowpoly_core::uv` (same names, snake_case). `projectUVs` and `wallUVs` come with the ops in R3. |
| `HalfEdgeMesh` + `Vertex`/`HalfEdge`/`Edge`/`Loop`/`Face` objects | `lowpoly_core::mesh::EditableMesh` with `Copy` handles `VertId`, `HalfEdgeId`, `EdgeId`, `LoopId`, `FaceId` |
| `vertex.pos`, `he.origin`, `he.twin`, `he.next`, `he.face`, `he.uv`, `edge.he`, `edge.hard` | `position(v)`, `origin(h)`, `twin(h)`, `next(h)`, `face_of(h)`, `uv(h)`, `edge_half_edge(e)`, `shading(e)` |
| `vertex.out` | `outgoing(v)` (iterator) |
| `edge.faces()`, `isWire`, `isBoundary` | `edge_faces(e)`, `is_wire(e)`, `is_boundary(e)` |
| `loop.halfEdges()`, `verts()`, `positions()`, `isHole` | `loop_half_edges(l)`, `loop_verts(l)`, `loop_positions(l)`, `is_hole(l)` |
| `face.outer`, `face.holes`, `face.loops`, `face.normal()` | `face_outer(f)`, `face_holes(f)`, `face_loops(f)`, `face_normal(f)` |
| `findEdge`, `findHalfEdge`, `edgeCount` | `find_edge`, `find_half_edge`, `edge_count` |
| `addVertex`, `removeVertex`, `addEdge`, `removeEdge`, `addFace`, `removeFace` | same names, snake_case; `add_face(&loops, material)`, `remove_face(f, RemoveFace { .. })` |
| `splitEdge`, `splitFace`, `joinFaces`, `addHole`, `fillHole`, `removeHole` (free functions) | `EditableMesh` methods with the same names; `add_hole(f, corners, HoleFill::Open \| Filled)` |
| `validate(mesh)`, `assertValid` | `mesh.validate()`, `mesh.assert_valid()` |
| `MeshError` (string messages) | `MeshError` enum, one variant per failure |
| `MeshData` types, `fromMeshData`, `toMeshData` | `lowpoly_core::mesh_data::MeshData` (serde, same JSON shape), `EditableMesh::from_data`, `EditableMesh::to_data` |
| `cloneMesh`, `cloneMeshData` | `Clone` (structurally shared) |
| `triangulatePolygon`, `triangulateFace` | `triangulate_polygon`, `Triangulator::triangulate_face` (reusable buffers) |

### Test mapping

All TS tests in `core/math/vec.test.ts`, `core/halfedge/*.test.ts` and `core/triangulate.test.ts`
are ported, one Rust test per TS test with a matching name:

- `vec.test.ts` → unit tests in `src/math.rs`
- `mesh.test.ts` → `tests/mesh.rs`
- `validate.test.ts` → unit tests in `src/mesh/validate.rs` (they corrupt private fields)
- `euler.test.ts` → `tests/euler.rs`
- `convert.test.ts` → `tests/mesh_data.rs` (the eight `rejects invalid data` cases are separate tests)
- `triangulate.test.ts` → `tests/triangulate.rs`

Adapted or replaced, because the behaviour they test no longer exists:

| TS test | Rust |
|---|---|
| validate: "detects broken twin pointers" | Dropped: twins are `index ^ 1`, so they can't disagree. |
| validate: "detects stale id counters" | `detects_stale_live_counts` (the counters are now live-element counts). |
| mesh: "assigns increasing ids and keeps counters past explicit ids" | `assigns_increasing_ids`: no explicit ids. |
| mesh: "copies positions instead of aliasing the input" | Dropped: positions are `Copy` values. |
| mesh: "rejects vertices from another mesh" | `rejects_vertices_that_are_not_live`: handles are plain indices, so a handle from another mesh can't be detected; a dead or out-of-range one can. |
| euler: "honours an explicit vertex id and rejects a used one" | `split_edge_rejects_a_dead_edge`. |
| euler: "rolls back completely when a rebuild fails partway" (forced with a clashing face id) | `add_hole_leaves_the_mesh_unchanged_when_refused`, plus `transaction_rolls_back_every_change_on_error` for partial rebuilds. |
| convert: "keeps nextVertId/nextFaceId at least past existing ids" | `remaps_sparse_file_ids_to_dense_handles`. |
| convert: "cloneMesh / cloneMeshData produce independent copies" | `clones_are_independent`. |

New in Rust: transactions, handle non-reuse, shading through `split_edge`, `split_face` with a
path (UV continuation and the planarity check), soft-edge round trips, the JSON shape, and
`CowVec` sharing.

### Deliberate differences

- **No explicit ids.** Handles are storage indices, allocated in order and never reused. The TS
  operators' optional `id` parameters are gone. Rebuilt faces still keep their ids.
- **File ids are remapped on load.** `from_data` assigns dense handles in file order, and
  `to_data` writes handles as ids. Files with dense ids in order (everything the TS app writes from
  a fresh load) round-trip exactly; sparse ids come back renumbered. `nextVertId`/`nextFaceId` are
  written as slot counts and ignored on load.
- **Rollback by transaction.** Compound operators run on a structurally shared copy and commit
  on success, instead of hand-written rollback.
- **No `edgeKey` string index.** `find_edge` walks the out list, which is O(valence).
- **`addHole` on a non-coplanar loop** reports `NotCoplanar` (the TS message was "Hole vertices
  must lie in the plane of the face").

### Preserved quirks

- `split_edge` measures `t` from the origin of the edge's primary half-edge, which depends on the
  direction the edge was first created in.
- `join_faces` keeps the id, material and endpoint UVs of the face on the primary half-edge's side.
- `remove_face` resets the UVs of the freed half-edges to (0, 0).

### Verification

`cargo test --workspace` (98 tests), `cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo clippy --workspace --target wasm32-unknown-unknown -- -D warnings` all pass.
