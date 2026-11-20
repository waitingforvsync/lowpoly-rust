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
