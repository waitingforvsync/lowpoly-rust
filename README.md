# Lowpoly (Rust)

A minimalist low-poly mesh editor, for desktop and the web. A Rust port of the
[TypeScript Lowpoly](https://github.com/waitingforvsync/lowpoly), built on eframe/egui, wgpu
(WebGL2 on the web) and glam.

**Live:** https://waitingforvsync.github.io/lowpoly-rust/ (deployed from `main` on every push).

- [PLAN.md](PLAN.md): design, architecture and roadmap
- [JOURNAL.md](JOURNAL.md): engineering decisions and mistakes along the way

## Building

One-time setup:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
```

Then:

```sh
cargo run -p lowpoly-app --release     # desktop
trunk serve                            # web dev server at http://127.0.0.1:8080, rebuilds on change
trunk build --release                  # static site in dist/ (relative URLs, host anywhere)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p lowpoly-app --target wasm32-unknown-unknown -- -D warnings
```

On Windows, run cargo from PowerShell or cmd rather than Git Bash: Git Bash's `/usr/bin/link`
shadows the MSVC linker.

## Navigation

Middle-drag orbits, Shift+middle or right-drag pans, the wheel zooms towards the pointer,
`5` toggles perspective / orthographic.
