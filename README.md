# Lowpoly (Rust)

A minimalist low-poly mesh editor, for desktop and the web. A Rust port of the
[TypeScript Lowpoly](https://github.com/waitingforvsync/lowpoly), built on eframe/egui, wgpu
(WebGL2 on the web) and glam.

**Live:** https://waitingforvsync.github.io/lowpoly-rust/ (deployed from `main` on every push).

- [PLAN.md](PLAN.md): design, architecture and roadmap
- [JOURNAL.md](JOURNAL.md): engineering decisions and mistakes along the way
- [PORTING.md](PORTING.md): how the TypeScript code maps to the Rust port, and what differs

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
trunk build --release --cargo-profile wasm-release   # static site in dist/, as deployed
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --target wasm32-unknown-unknown -- -D warnings
cargo bench -p lowpoly-core --bench mesh     # mesh benchmarks (criterion)
```

On Windows, run cargo from PowerShell or cmd rather than Git Bash: Git Bash's `/usr/bin/link`
shadows the MSVC linker.

## Navigation

Middle-drag orbits, Shift+middle or right-drag pans, the wheel zooms towards the pointer,
`5` toggles perspective / orthographic.
