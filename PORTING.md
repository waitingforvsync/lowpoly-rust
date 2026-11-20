# Porting notes

How the TypeScript app (`../lowpoly`) maps to this Rust port, milestone by milestone. The TS
code is the behavioural spec; the Rust architecture differs on purpose (see [PLAN.md](PLAN.md)).

## R1 — Mesh core

### API mapping

| TypeScript | Rust |
|---|---|
| `math/vec.ts` (tuple vectors) | `glam::DVec2` / `DVec3`; `lowpoly_core::math` for `newell_normal` (takes any iterator), `plane_basis`, `signed_area_2d`, `point_in_polygon_2d`, plus `PlaneProjector` (was a private helper in `euler.ts`) |
| `planarity.ts` | `lowpoly_core::planarity` (`planar_deviation`, `planar_tolerance`, `is_planar`; all take cloneable iterators) |
| `uv/project.ts`, `uv/continue.ts` (`planarAxes`, `planarUV`, `faceUVMap`, `faceUVFunction`) | `lowpoly_core::uv`: `planar_axes`, `planar_uv`, `face_uv_map` (returns an `AffineUvMap`), `face_uv_mapping` (returns a `UvMapping` enum, not a closure). `projectUVs` and `wallUVs` come with the ops in R3. |
| `HalfEdgeMesh` + `Vertex`/`HalfEdge`/`Edge`/`Loop`/`Face` objects | `lowpoly_core::mesh::EditableMesh` with `Copy` handles `VertId`, `HalfEdgeId`, `EdgeId`, `LoopId`, `FaceId` |
| `vertex.pos`, `he.origin`, `he.next`, `he.face`, `he.uv`, `edge.hard` | `position(v)`, `origin(h)`, `next(h)`, `face_of(h)`, `uv(h)`, `shading(e)` |
| `he.twin`, `he.edge`, `edge.he` | `h.twin()`, `h.edge()`, `e.half_edge()`: methods on the handles, since they are index arithmetic |
| `vertex.out` | `outgoing(v)` (iterator) |
| `edge.faces()`, `isWire`, `isBoundary` | `edge_faces(e)`, `is_wire(e)`, `is_boundary(e)` |
| `loop.halfEdges()`, `verts()`, `positions()`, `isHole` | `loop_half_edges(l)`, `loop_verts(l)`, `loop_positions(l)` (iterators), `is_hole(l)` |
| `face.outer`, `face.holes`, `face.loops`, `face.normal()` | `face_outer(f)`, `face_holes(f)`, `face_loops(f)`, `face_normal(f)` |
| `findEdge`, `findHalfEdge`, `edgeCount` | `find_edge`, `find_half_edge`, `edge_count` |
| `addVertex`, `removeVertex`, `addEdge`, `removeEdge`, `addFace`, `removeFace` | same names, snake_case; `add_face(&loops, material)`, `remove_face(f, RemoveFace { .. })` |
| `splitEdge`, `splitFace`, `joinFaces`, `addHole`, `fillHole`, `removeHole` (free functions) | `EditableMesh` methods with the same names; `add_hole(f, corners, HoleFill::Open \| Filled)` |
| `validate(mesh)`, `assertValid` | `mesh.validate()`, `mesh.check()` |
| `MeshError` (string messages) | `MeshError` enum, one variant per failure |
| `MeshData` types, `fromMeshData`, `toMeshData` | `lowpoly_core::mesh_data::MeshData` (serde, same JSON shape), `EditableMesh::from_data`, `EditableMesh::to_data` |
| `cloneMesh`, `cloneMeshData` | `Clone` (structurally shared) |
| `triangulatePolygon`, `triangulateFace` | `triangulate_polygon`, `Triangulator::triangulate_face` / `triangulate_face_into` (reusable buffers) |

### Test mapping

All TS tests in `core/math/vec.test.ts`, `core/halfedge/*.test.ts` and `core/triangulate.test.ts`
are ported, one Rust test per TS test with a matching name:

- `vec.test.ts` → unit tests in `src/math.rs`
- `mesh.test.ts` → `tests/mesh.rs`
- `validate.test.ts` → unit tests in `src/mesh/validate.rs` (they corrupt private fields)
- `euler.test.ts` → `tests/euler.rs`
- `convert.test.ts` → `tests/mesh_data.rs` (the eight `rejects invalid data` cases are separate
  tests), except the clone tests, which became `clones_are_independent` in `tests/mesh.rs`
- `triangulate.test.ts` → `tests/triangulate.rs`

Adapted or replaced, because the behaviour they test no longer exists:

| TS test | Rust |
|---|---|
| validate: "detects broken twin pointers" | Dropped: twins are `index ^ 1`, so they can't disagree. |
| validate: "detects stale id counters" | `detects_stale_live_counts` (the counters are now live-element counts). |
| mesh: "assigns increasing ids and keeps counters past explicit ids" | `assigns_increasing_ids`: no explicit ids. |
| mesh: "copies positions instead of aliasing the input" | Dropped: positions are `Copy` values. |
| mesh: "rejects vertices from another mesh" | `rejects_vertices_that_are_not_live` and `rejects_handles_of_removed_elements`: handles are plain indices, so a handle from another mesh can't be detected; a dead or out-of-range one can. |
| euler: "honours an explicit vertex id and rejects a used one" | `split_edge_rejects_a_dead_edge`. |
| euler: "rolls back completely when a rebuild fails partway" (forced with a clashing face id) | `add_hole_rolls_back_completely_when_the_rebuilt_face_is_invalid`: the rebuilt face repeats a vertex, which passes the up-front checks and is caught before anything changes. `transaction_rolls_back_every_change_on_error` covers multi-step edits. |
| convert: "keeps nextVertId/nextFaceId at least past existing ids" | `remaps_sparse_file_ids_to_dense_handles`. |
| convert: "cloneMesh / cloneMeshData produce independent copies" | `clones_are_independent`. |

New in Rust: every operator's error paths (each refusal checked to leave the mesh unchanged and
valid, via `assert_refused`), non-finite input, validate's bounded walks and extra checks, a
property test of random operator sequences (`tests/properties.rs`), a literal TS-written JSON
document, dead-element compaction, handle arithmetic, and `CowVec` sharing.

### Bugs fixed (present in the TS app)

- **`joinFaces` accepted non-coplanar faces** when their concatenated loops cancelled in the
  Newell sum: the shared edge appears twice, and for two triangles every edge cancels, giving a
  zero normal that planarity treated as "degenerate, so planar". Found by the property test.
  `join_faces` now tests the merged face it would build, and `planar_deviation` falls back to a
  well-spread triangle when the Newell normal is degenerate.
- **NaN and infinite input passed every precondition** (`isPlanar` treated a NaN normal as
  degenerate). Non-finite points are never planar, and `split_edge`/`split_face` reject non-finite
  input with `MeshError::NonFinite`.
- **Validation could hang** on a loop or hole chain that cycles without returning to its start
  (the planarity check walked loops without a bound). Every walk is now bounded.
- **Validation missed a half-edge claiming a loop it isn't on**, which made `isBoundary` and
  `faces()` wrong.
- **Small faces lost texture continuation**: `faceUVMap` compared side⁴-sized quantities against
  an absolute epsilon. The tolerances now scale with the face.

### Deliberate differences

- **No explicit ids.** Handles are storage indices, allocated in order and never reused. The TS
  operators' optional `id` parameters are gone. Rebuilt faces still keep their ids.
- **File ids are remapped on load.** `from_data` assigns dense handles in file order, and
  `to_data` writes handles as ids. Files with dense ids in order and counters equal to the counts
  round-trip exactly; anything else comes back renumbered. `nextVertId`/`nextFaceId` are written
  as slot counts and ignored on load.
- **Atomic operators without rollback.** Each operator checks everything that could fail
  (including the faces it would rebuild) before changing anything, instead of rebuilding and
  rolling back on failure. `transaction` remains for multi-step edits.
- **Edge directions survive undo.** TS undo rebuilds the half-edge mesh from `MeshData`, which
  resets edge directions to face order. Undo here restores the mesh value itself, so directions
  (which decide `split_edge`'s `t` and which face `join_faces` keeps) are preserved. They are
  reset on save and load, as in TS.
- **No `edgeKey` string index.** `find_edge` walks the out list, which is O(valence).
- **Errors are typed.** `addHole` on a non-coplanar loop reports `NotCoplanar` (the TS message was
  "Hole vertices must lie in the plane of the face"); load errors name the face or edge that refers
  to an unknown vertex.

### Preserved quirks

- `split_edge` measures `t` from the origin of the edge's primary half-edge, which depends on the
  direction the edge was first created in.
- `join_faces` keeps the id, material and endpoint UVs of the face on the primary half-edge's side.
- `remove_face` resets the UVs of the freed half-edges to (0, 0).
- UV rounding rounds halves up, as `Math.round` does.

### Verification

`cargo test --workspace` (138 tests, including 256 random operator sequences; 20,000 were also run
once), `cargo clippy` with `-D warnings` for native and wasm32, and `cargo doc` with `-D warnings`
all pass.
