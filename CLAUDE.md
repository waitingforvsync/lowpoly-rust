# Lowpoly (Rust) — project rules

A Rust port of the TypeScript editor in `../lowpoly`. Read [PLAN.md](PLAN.md) for the architecture,
[PORTING.md](PORTING.md) for how the TS code maps over, and [JOURNAL.md](JOURNAL.md) for past
decisions. Keep all three up to date as milestones land.

## Coding standards

- Follow the `rust-best-practices` skill (BeautifulRust). Lints are configured in the workspace
  `Cargo.toml` and `clippy.toml`; CI runs clippy with `-D warnings` for native and wasm32.
- **Functional style where possible.** Prefer expressions over statements, iterator pipelines
  over index loops, and the monadic APIs of `Option`/`Result` (`map`, `and_then`, `ok_or`,
  `filter`, `?`, `let … else`) over manual matching and mutable accumulators. Keep an explicit
  loop when it is genuinely clearer (several jobs per iteration, awkward early exits) or measurably
  faster in a hot path.
- **Architecture first.** Don't transliterate TS idioms; redesign around ownership and values
  (see PLAN.md). Performance on web and native is a primary goal: avoid needless allocation and
  copies in hot paths, and measure with the criterion benches before and after optimising.
- Port behaviour test-first: TS tests become Rust tests with matching names.

## Workflow

- On Windows, run cargo and trunk from PowerShell, not Git Bash (Git Bash's `/usr/bin/link`
  shadows the MSVC linker).
- Before handing back: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo clippy --workspace --target wasm32-unknown-unknown -- -D warnings`, `cargo test --workspace`.
