# Engineering Journal

A running record of design decisions, the reasoning behind them, and mistakes made along the
way (with what they taught). See [PLAN.md](PLAN.md) for the roadmap.

---

## R0 — Scaffold and deploy (2026-11-20)

### Decisions
- **Workspace of three crates** (`lowpoly-core`, `lowpoly-editor`, `lowpoly-app`), so everything
  but drawing and platform glue is tested on the host. The core is still empty at this point.
- **Offscreen viewport.** The scene renders into our own `Rgba8Unorm` colour + `Depth32Float` depth
  (4× MSAA when the adapter supports it) and egui shows the resolved texture. egui samples native
  textures as gamma-space `Rgba8Unorm`, so shaders write sRGB-encoded colour directly.
- **Reverse-Z depth**: infinite far plane in perspective, swapped near/far in orthographic. Depth
  clears to 0 and tests with `GreaterEqual`.
- **Camera matrices are hand-written in f64.** glam 0.34 moved its projection and look-at
  constructors into an f32-only `glam::camera` module. The camera keeps its state and maths in f64;
  matrices are converted with `as_mat4()` for the GPU.
- **Orthographic shows the same area as perspective at the target** (half height = distance ×
  tan(fov/2)), with the eye placed 500 units back so nothing is clipped, as in the TS app.
- **Re-render only on change.** The viewport keeps the last view-projection and size and skips
  the GPU work when neither changed; egui itself only repaints on input.
- **WebGL2 only on the web** for now (`Backends::GL` in `main.rs`); WebGPU is a one-line change.
- **Lints**: BeautifulRust's set as `[workspace.lints]`, minus the library-only lints and
  `as_conversions`; casts go through `From`/`TryFrom` or glam's `as_*` conversions, and the one
  float-to-pixel cast carries an `#[expect]` with a reason.

### Mistakes
- **Git Bash shadows the MSVC linker.** `/usr/bin/link` comes before `link.exe` on Git Bash's
  `PATH`, so every build script failed to link, including the first `cargo install trunk`, whose
  failure a `| tail` hid behind exit code 0. *Lesson: run cargo from PowerShell on Windows, and
  don't pipe an install's output through a filter that swallows its exit status.*
- **egui's eframe 0.37 API had moved on** from the version I knew: `App::ui(&mut Ui, …)` replaces
  `update`, and `Panel::top(…).show(ui, …)` replaces `TopBottomPanel`. Reading the registry source
  first saved several build iterations.
- **Wheel zoom stalled part-way.** egui spreads each wheel notch over several frames through
  `smooth_scroll_delta`, but doesn't keep repainting unless a scroll area consumes it. Fixed by
  requesting a repaint while `is_scrolling()`.
- **Synthetic browser input**: eframe reads mouse moves from `mousemove`, not `pointermove`. Browser
  tests must dispatch a `mousemove` before wheel or drag events, or egui has no hover position.

---

## R1 — Mesh core (2026-11-20)

### Decisions
- **Half-edges are allocated in twin pairs**: twin = `index ^ 1`, edge = `index / 2`. Twin and
  edge pointers can't be inconsistent, there's no edge-to-half-edge field, and an edge's primary
  half-edge keeps the TS meaning of `edge.he` (its creation direction).
- **Handles wrap `NonZeroU32`** (storing index + 1), so `Option<HalfEdgeId>` is 4 bytes and a
  half-edge record is 20 bytes. Faces chain their holes through the loops, and vertices thread
  their out lists through the half-edges, so every record is fixed-size and fits in a chunk.
- **One `CowVec` per attribute**: positions, vertex links, half-edge links, UVs, edges, loops,
  faces. Elements are append-only with an `alive` flag. Faces rebuilt by an operator reuse their
  slot, so they keep their id; their loops are new.
- **The chunk table is shared too** (`Arc<Vec<Arc<[T; 64]>>>`). Cloning a mesh is then O(1), and
  each attribute pays for a table copy only when first written. On a 100×100 grid: clone + move one
  vertex went from 18.7 µs to 1.5 µs, and a transactional `split_edge` from 39 µs to 21 µs.
  The cost: writes to an unshared mesh do two `Arc` uniqueness checks instead of one, so building
  the grid went from 2.6 ms to 4.5 ms. Clones happen on every edit, bulk builds only on load and
  import, so this is the right trade. A batch-edit scope that checks uniqueness once per chunk is
  the next step if write-heavy operations ever show up in profiles.
- **Transactions replace hand-written rollback**: `EditableMesh::transaction` runs an edit on a shared
  copy and commits on success. Euler operators check what they can up front, then rebuild faces
  inside a transaction.
- **`find_edge` walks the out list** (O(valence)) instead of keeping a vertex-pair hash map, which
  would need its own structurally shared storage.
- **File ids are remapped on load** to dense handles; `nextVertId`/`nextFaceId` become slot counts.
  See PORTING.md.
- **earcut** is the georust port of mapbox earcut 3.x, the same algorithm as the TS app, with
  reusable buffers (`Triangulator`).
- **Lints**: `many_single_char_names` and `similar_names` are allowed workspace-wide; geometry code
  reads best with `u`, `v`, `n`, `a`, `b` and paired names like `loop_a`/`loops_a`.

### Mistakes
- A ported test moved a cube corner and then asserted the copy was still valid. It isn't (faces
  bend), and `validate` said so. The test now checks independence only.
- **Module layout**: the mesh started as one 800-line `mesh/mod.rs`. Moved to the modern
  convention (`mesh.rs` holding only `mod` and `pub use`, items in `mesh/*.rs`), and renamed the type
  `EditableMesh` (in `mesh/editable_mesh.rs`) so the module root stays a thin index. Storage records
  and fields are `pub(super)` so the sibling files (primitives, Euler operators, validation) can
  reach them.
- `cargo bench -p lowpoly-core -- <criterion flags>` passes the flags to the lib's test harness
  too, which rejects them. Use `--bench mesh`.
