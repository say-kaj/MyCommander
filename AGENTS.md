# AGENTS.md

Guidance for coding agents (and humans) working on myCommander.

- **What it is:** native macOS two-panel file manager modelled on Midnight Commander. Rust + egui/eframe + muda.
- **Spec:** [`docs/SPEC.md`](docs/SPEC.md) is normative. Read it first; cite requirement IDs (e.g. `PNL-9`) in commits.
- **Language:** all code, comments, docs and commits are English.

## Commands

```sh
cargo build --release        # binary: target/release/myCommander
cargo run --release -- [left-dir] [right-dir]
cargo test
./target/release/myCommander --screenshot out.png   # render one frame and exit
MYCMD_SHOT_MENU=2 ./target/release/myCommander --screenshot menu.png
```

## Layout

See SPEC §11. Pure logic: `panel.rs`, `fsutil.rs`. Background work: `ops.rs`.
Single menu table: `menus.rs` (feeds both the in-window and the macOS menu bar).

## Rules

1. Behaviour in SPEC §4–§7 is normative. Change the spec first, then the code.
2. New menu item → add to `menus.rs`; don't list features that don't exist (no disabled placeholders).
3. New settings field → must have a serde default (CFG-5).
4. Long-running file work goes into `ops.rs` as a background job; never block the UI thread.
5. Never run destructive operations on real user paths in tests; use temp dirs.
6. Keep the build warning-free; no new dependencies without justification.
7. Don't reintroduce mc's terminal colours or terminal-only features (see non-goals).
