# Lowpoly (Rust) — Plan

A Rust port of the TypeScript low-poly mesh editor in `../lowpoly`
(live at https://waitingforvsync.github.io/lowpoly/). The port keeps the original's behaviour
and UX (SketchUp-style modelling, Z-up, strictly planar n-gon faces with holes) and runs both
natively and in the browser from one codebase.

- Remote: `github.com/waitingforvsync/lowpoly-rust`
- Web build: GitHub Pages from that repo, `https://waitingforvsync.github.io/lowpoly-rust/`
- The original's `PLAN.md`, `CODE.md` and `JOURNAL.md` remain the spec for *behaviour*. This file
  covers what changes in Rust and the order of work.

**Guiding principle: port the behaviour, not the structure.** Two goals drive the port: a
cleaner architecture, and better performance than JavaScript, both natively (instead of
JS-in-Electron) and on the web (wasm instead of JS). Wherever a TypeScript idiom exists only
because of JS (object graphs, exceptions, reactive signals, a retained scene graph, stringly-typed
keys), the Rust version is redesigned around data ownership, not transliterated. The original's
file and class layout is not a template.

## Decisions

| Topic | Decision |
|---|---|
| App shell | `eframe` + `egui` 0.37, wgpu backend on both targets |
| GPU | wgpu (through `eframe::wgpu`, never a separately versioned `wgpu` dependency). Native: Vulkan/DX12/Metal. Web: **WebGL2** via wgpu's `webgl` feature, so everything is written to `Limits::downlevel_webgl2_defaults()`. WebGPU can be enabled later as a feature flag. |
| Math | `glam`. **Core geometry in `f64` (`DVec3`/`DVec2`)**, converted to `f32` only when building GPU buffers. JS numbers are f64, and the planarity tolerance (`1e-5 × bbox diagonal`), snapping and repeated ops would drift in f32. `nalgebra` only if a solver needs it later. |
| Triangulation | `earcut` crate (Rust port of mapbox earcut, same algorithm as the original) |
| Serialization | `serde` + `serde_json`. **Same JSON document format as the TS app**, so files open in either, and TS-saved files become test fixtures. |
| Undo | Value-oriented: the document is an immutable-by-sharing value (`imbl` + `Arc`, chunked copy-on-write mesh arrays), history is a list of document values. See [Document model and undo](#document-model-and-undo). |
| Errors | One error enum per area (`MeshError`, `IoError`, ...) with `thiserror`. Mesh primitives check preconditions before mutating, as in the original. |
| Web bundling | `trunk` (`index.html` + `Trunk.toml`); `wasm-opt` in release |
| Lints | BeautifulRust clippy config as workspace `[lints]`; CI runs clippy with `-D warnings` for native and `wasm32-unknown-unknown` |
| Tests | Port the original's vitest suites test-first, as **behaviour** tests against the new API (names map back to the originals; tests of TS-only internals such as the MeshData↔HalfEdge conversion become file-format round-trip tests). `validate()` runs in every test and after every command in debug builds. |

## Workspace layout

```
Cargo.toml                 workspace, shared [lints], [profile.release]
crates/
  lowpoly-core/            pure model: no egui, no wgpu, no platform code
    math/                  plane fit (Newell), projection, ray/segment helpers
    mesh/                  Mesh (arena half-edge), handles, Euler ops, queries, validate
    file_format.rs         MeshData + document DTOs (serde), TS-compatible JSON
    ops/                   push/pull, extrude, inset, loop cut, merge, dissolve, planarize, draw, ...
    uv/                    projection, texture-lock continuation
    triangulate.rs
    primitives.rs
    document.rs            objects, materials, texture assets
    cow_vec.rs             chunked copy-on-write array (structural sharing for meshes)
    history.rs             value history: entries of Document + selection, cursor, budget
    io/                    JSON, OBJ/MTL, GLB, zip (byte-in/byte-out, no file system)
  lowpoly-editor/          session logic, still pure and natively testable
    editor.rs              EditorState: mode, selection, active tool, snap settings
    selection.rs, commands.rs, actions/*.rs, keymap.rs
    camera.rs              orbit/pan/zoom, views, ortho/persp (glam)
    picking.rs             CPU ray/triangle and projected vertex/edge picking
    inference.rs, measure.rs, tools/*.rs (draw, face drag, gizmo logic)
  lowpoly-app/             the binary: eframe App, renderer, panels, platform glue
    main.rs                native entry + wasm entry (cfg'd)
    render/                wgpu pipelines, mesh buffers, overlays, grid, gizmo drawing, UV view
    ui/                    toolbar, outliner, properties, status bar, UV editor (egui)
    platform/              file open/save, downloads, autosave storage (native vs web)
index.html, Trunk.toml     web entry
.github/workflows/         ci.yml (check + test), pages.yml (deploy)
```

Dependencies point downwards (`app → editor → core`), mirroring the original's layers. The split
exists so that everything except drawing and platform I/O is unit-tested with plain `cargo test`
on the host, and so the core cannot accidentally pick up a GPU or browser dependency.

### Architecture: what is redesigned rather than ported

| TS original | Why it existed | Rust design |
|---|---|---|
| `MeshData` does two jobs: the file format, **and** the in-memory snapshot form for undo (the half-edge graph is converted to `MeshData` on every mesh edit and rebuilt from it on every undo) | A JS object graph of half-edges can't be cheaply cloned | **Still two forms, each with one job.** `MeshData` is only the serialized format (serde, TS-compatible), used at save/load/export. The runtime `Mesh` is index-based half-edge storage with structural sharing (see [Document model and undo](#document-model-and-undo)), so it is its own undo snapshot. No conversion on the edit/undo path. |
| Command pattern: each command mutates the live document in `apply()` and hand-writes its inverse in `revert()` | OO idiom; no cheap snapshots | **Value-oriented history** (after Juan Pedro Bolívar Puente's immer/lager): the document is a value, history is a list of document values. No inverses to write or get wrong. |
| Throw `MeshError`, catch, roll back pointer surgery | Exceptions | Ops return `Result<_, MeshError>` and run on a working copy of the document (cheap thanks to sharing). On error the copy is dropped, so a failed op can't leave a half-edited mesh, by construction. |
| Preact signals; panels re-render on change | Reactive UI | egui immediate mode reads editor state each frame. UI events become an `Action` enum handled by the editor (also useful for tests and key bindings). |
| `SceneSync` keeps a Three.js scene graph in sync with the document | Retained-mode renderer | No scene graph. The renderer caches GPU buffers, triangulation and normals per mesh, and detects change with `Arc::ptr_eq` (a pointer compare). Draw lists are rebuilt from the document each frame, which is cheap. |
| Live previews mutate the document, then `notifyPreview()`; commit must reconstruct "before" | Shared mutable objects | A tool holds `preview: Option<Document>`; the renderer draws the preview if present, else the current document. Commit pushes it to history, cancel drops it. |
| Tool classes with event callbacks and ad hoc flags | DOM event model | Each tool is an explicit **state machine** (`enum` states, `fn update(&mut self, input, &EditorState) -> ToolOutcome`), driven from egui input. Easy to unit-test without a GPU. |
| String keys (`"a-b"` edge ids, action names, mode strings) | JS maps keyed by string | Typed keys (`EdgeKey(VertId, VertId)` normalised so the smaller id comes first), enums for modes, tools and actions. Ordered collections where iteration order is observable. |
| Everything in `number` (f64) | JS | `f64` geometry in the model (keeps tolerances and repeated ops stable); `f32` only in GPU buffers. |

## Document model and undo

### Document as a value

```rust
#[derive(Clone)]
struct Document {
    objects: imbl::Vector<Arc<Object>>,
    materials: imbl::OrdMap<MaterialId, Arc<Material>>,
    textures: imbl::OrdMap<TextureId, Arc<TextureAsset>>, // image bytes shared, never copied
}
struct Object { name: String, transform: Transform, visible: bool, locked: bool, mesh: Arc<Mesh>, source: Option<PrimitiveSource> }
```

- Cloning a `Document` copies pointers only. Edits go through `Arc::make_mut` / `imbl`'s copy on
  write: a value owned by one holder is changed in place, and a shared one is copied first.
- Modelling ops stay ordinary imperative code on `&mut Mesh`. Value semantics apply only at the
  document boundary.

### History

```rust
struct Entry { doc: Document, sel_before: Selection, sel_after: Selection, label: &'static str }
struct History { entries: Vec<Entry>, cursor: usize, saved: Option<Document> }
```

- **Execute:** clone the current document, apply the op to the clone, push it (truncating the redo
  branch). On error nothing is pushed.
- **Undo/redo:** move the cursor; O(1).
- **Coalescing** (drags, measurement-box retype in the same tool): replace the top entry.
- **Selection is not part of the document** and selecting creates no entry, as in the TS app. Each
  entry records the selection before and after, and undo/redo restores it. (It can move into the
  document later if we want undoable selection.)
- **Dirty flag:** current document `ptr_eq` the saved one, with no depth bookkeeping.
- **Limit:** a memory budget (estimated from unshared chunks) rather than a fixed entry count.
- **Autosave** serializes a snapshot without blocking editing (worker thread natively, idle
  slices on the web).

### Mesh storage

- **Index-based throughout:** typed `u32` handles (`VertId`, `HalfEdgeId`, `EdgeId`, `LoopId`,
  `FaceId`), no pointers. An index means the same element in every version, which is what
  lets versions share storage.
- **`CowVec<T, const N: usize>`**: a two-level chunked array, `Vec<Arc<[T; N]>>`. Reads are one extra
  indirection (O(1), cache-friendly). The first write to a shared chunk copies only that chunk; a new
  version copies only the chunk table (~5 KB for 40k elements). Chunk size is tunable (start at 64).
  Chosen over `imbl::Vector` for meshes because half-edge traversal is dominated by random reads
  and RRB tree walks would sit on the hot path. `imbl` is used at the document level only.
- **One `CowVec` per attribute** (struct of arrays): positions, vertex links, half-edge
  connectivity, half-edge UVs, edge flags, loops, face attributes. Common edits touch one array:
  vertex moves copy only position chunks, UV edits only UV chunks, painting only face chunks.
- **Append-only allocation with tombstones.** New elements go in fresh tail chunks that no older
  version references, so creating geometry never copies. Deleted elements are marked dead and not
  reused. Compaction happens only on load (or an explicit cleanup).
- **Indices are the stable ids.** Because elements never move, selection and history use the
  indices directly, and no id→index map is needed. The TS file format's ids are remapped on load
  and written from indices on save.
- **Per-vertex outgoing half-edges** (the TS design's unordered `out` list, needed because
  boundary half-edges have no next/prev) become an intrusive linked list: each half-edge stores
  `next_out`, each vertex stores `first_out`. Everything stays fixed-size and fits in chunks.
- **Cost model:** an edit copies the chunks it touches and never more than a full snapshot. A typical
  push/pull on a 10k-quad mesh copies tens of KB instead of ~2 MB. Global ops (apply scale,
  recalculate normals) pay a full copy, like any scheme.

### Performance

- **Data-oriented mesh storage** as above: contiguous chunks, struct of arrays, no per-element heap
  allocations.
- **Incremental GPU updates** keyed by mesh identity (`Arc::ptr_eq`); triangulation and normals
  recomputed per changed mesh only. Vertex dots and edge quads are instanced.
- **Picking** stays on the CPU (WebGL2 readback is slow and synchronous). Per-object bounding boxes
  cull first; add a BVH per mesh only if profiling asks for it.
- **Render on demand**: `request_repaint` only on input, change or animation, so an idle editor
  uses no CPU or GPU.
- **Benchmarks** (`criterion`) for the hot paths in `lowpoly-core`: triangulation, conversion, picking,
  push/pull on dense meshes. Target: smooth interaction on meshes much larger than the TS app handles.
- Single-threaded by design (wasm). Core types stay `Send + Sync` so native-only parallelism (e.g.
  `rayon` for import or triangulation) can be added later without restructuring.

## Rendering

- The 3D viewport renders **offscreen** into our own colour + depth (+ MSAA resolve) textures,
  registered with egui via `egui_wgpu::Renderer::register_native_texture` and shown as an image
  filling the central panel. That gives us control of depth, MSAA, and render passes independent of
  egui's surface, and allows headless render tests. (Alternative: an `egui_wgpu` paint callback;
  fewer copies but tied to egui's pass and depth settings.)
- 2D labels on top of the viewport (inference tooltips, measurement hints, axis labels) are drawn
  with egui's painter, not in wgpu.
- **WebGL2 constraints to design for from day one:** no compute, no storage buffers (uniforms only,
  ≤16 KB blocks), no `PolygonMode::Line`, line primitives are always 1 px. So wireframe and
  edge overlays are line-list geometry, and thick edges/vertex dots are screen-space quads expanded
  in the vertex shader. WGSL only, kept within what naga translates to GLSL ES 3.0.
- Pipelines: faces (flat + soft-edge normals, per-material colour/texture, X-ray and backface
  variants), edges, vertices, grid/axes, highlights, gizmo. Picking is CPU-side (as in the original).

## Platform layer (native vs web)

| Need | Native | Web |
|---|---|---|
| Open / import files | `rfd` dialog, `std::fs` | `rfd` async dialog (bytes), egui dropped files |
| Save / export | `rfd` save dialog | Blob download via `web-sys` |
| Autosave | file in the platform data dir | **IndexedDB** (localStorage's ~5 MB limit is too small for embedded textures) |
| Time | `std::time` | `web-time` |
| Logging / panics | `env_logger` | eframe's web logger + panic hook (shows an error in the page) |

Behind a small `Platform` trait / cfg'd module, so the editor never sees the difference.
File I/O in `lowpoly-core::io` works on bytes only.

## Build, run, deploy

```sh
cargo run -p lowpoly-app                 # native
trunk serve                              # web dev server with reload (http://127.0.0.1:8080)
trunk build --release                    # static site in dist/
cargo test --workspace                   # host tests (core + editor)
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p lowpoly-app --target wasm32-unknown-unknown -- -D warnings
```

- One-time setup: `rustup target add wasm32-unknown-unknown`, `cargo install trunk --locked`
  (neither is installed yet). `.cargo/config.toml` can alias the long commands.
- `trunk build --public-url ./` (relative), so the site works from `/lowpoly-rust/` or anywhere.
- **CI** (`ci.yml`, every push and PR): fmt check, clippy native + wasm32, `cargo test`.
- **Pages** (`pages.yml`, push to `main`): install wasm target + trunk (cached), `trunk build
  --release`, `upload-pages-artifact`, `deploy-pages`, as in the original's workflow.
  Requires repo Settings → Pages → Source: "GitHub Actions" (one-time, in the GitHub UI).
- Later, optional: native release binaries (Windows/macOS/Linux) on tags.

## Milestones

The core is pure and has a large test suite, so it's ported in bulk before the UI is rebuilt on top.
The pipeline goes first so every later milestone is deployed and checked in the browser as it lands.

**Phase A: pipeline**

- **R0 Scaffold and deploy.** Workspace, lints, eframe app with the offscreen wgpu viewport:
  Z-up grid and axes, orbit/pan/zoom camera, placeholder panels (toolbar, outliner, properties,
  status bar). Runs natively and via trunk under WebGL2. CI + Pages deploy live. `git init`, push to
  the remote. **Exit:** the same build runs on desktop and at the Pages URL.

**Phase B: core port (test-first, no UI)**

- **R1 Mesh core.** Math, arena `Mesh` with holes, Euler ops, `validate`, triangulation, the
  file-format DTO. Port `halfedge/*.test.ts`, `triangulate.test.ts`. First benchmarks.
- **R2 Document, history, primitives, JSON I/O.** Port `history.test.ts`; load TS-saved JSON
  fixtures and round-trip them.
- **R3 Modelling ops.** Planarize/fold, push/pull, extrude, inset, split/connect, loop cut, merge,
  dissolve, make face/hole, normals, triangulate, mirror, draw ops, UV projection and texture lock.
  Port `ops/*.test.ts`, `uv` tests.
- **R4 OBJ / GLB / zip.** Port `io.test.ts`. Differential check: export from both apps on the same
  documents and compare.

**Phase C: editor and app (feature by feature, each deployed)**

- **R5 Objects.** Mesh rendering with materials, object picking and selection, outliner,
  properties, gizmo (port the original's custom gizmo), undo/redo, save/open, autosave.
- **R6 Component editing.** Vertex/edge/face modes, overlays, hover, box select (window/crossing),
  component transforms with folding, delete/dissolve/merge. Port `commands.test.ts`.
- **R7 Modelling tools.** Push/pull and face drag, extrude, inset, connect, loop cut, make face.
- **R8 Materials and UVs.** Textured rendering, texture import, paint tool, UV projection UI,
  UV editor panel.
- **R9 SketchUp feel.** Inference snapping, axis lock, measurement box, line and rectangle tools.
- **R10 Object ops, selection commands, polish.** Join/separate, apply transform, set origin,
  select linked/grow/shrink/edge loop, full keymap, OBJ import by drop, export. **Exit:** feature
  parity with the TS app, signed off against its shortcut and operations lists.

## Documentation

As in the original: `PLAN.md` (this), `CODE.md` (structure and APIs, written as code lands),
`JOURNAL.md` (decisions and mistakes per milestone), plus `PORTING.md` (API and test mapping from
TS to Rust, deliberate differences, preserved quirks).

## Risks

- **egui vs Preact UI.** Immediate-mode panels will look and lay out differently. Aim for the same
  functionality and keyboard behaviour, not a pixel match.
- **WebGL2 limits** on line rendering and uniforms (addressed in Rendering above).
- **Browser key reservations** (Ctrl+N, Ctrl+W, ...) and egui's own shortcuts on web; check the
  keymap early in R5.
- **wasm size**: egui + wgpu + image decoding is several MB; use `wasm-opt -Oz`, and only the
  `image` codecs we need (PNG, JPEG).
