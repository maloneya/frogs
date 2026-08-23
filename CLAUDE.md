# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

A from-scratch action-RPG engine, built as a **learning project**. The goal is
understanding engine-level technology, not shipping a game. Specifically: making
combat against a large horde of enemies *feel* right.

This framing changes how to work here:

- **Explain, don't just deliver.** Concise natural-language reasoning about why
  a design is what it is matters more than the code being finished.
- **Small chunks.** Build one system at a time and get it running before moving
  on. Do not scaffold several subsystems at once, and do not one-shot features.
- **First principles.** Engine layers get hand-written — the sim loop, entity
  storage, spatial partitioning, the renderer. Reach for a crate for math and
  plumbing, not for the layers being studied. In particular, do not introduce a
  game engine or an off-the-shelf ECS (Bevy, hecs, legion); writing those is the
  point.

## Commands

```sh
cargo run                 # debug build, run
cargo run --release       # for any performance measurement — debug numbers are meaningless
RUST_LOG=info cargo run   # adapter selection + wgpu diagnostics
cargo clippy              # kept warning-clean
```

Rust was installed via rustup with `--no-modify-path`, so `~/.cargo/bin` is
**not** on PATH by default. Prefix commands with `. "$HOME/.cargo/env" &&`, or
add it to the shell profile.

`[profile.dev.package."*"] opt-level = 3` in Cargo.toml optimises dependencies
while leaving this crate in debug. Graphics crates are unusably slow otherwise.

## Architecture

Native macOS only (Apple M4 / Metal). Cross-platform and wasm support are
explicit non-goals — a lot of wgpu example code exists to satisfy the browser's
ban on blocking the main thread, and none of that complexity is warranted here.

- `src/main.rs` — winit `ApplicationHandler`. Owns the event loop and window
  lifecycle. GPU state is built in `resumed`, not `main`, because winit models
  surface loss as suspend/resume. `about_to_wait` requests a redraw every time
  the queue drains, which is what converts winit's event-driven default into a
  continuous game loop.
- `src/gfx.rs` — `Renderer`: wgpu instance/surface/device/queue plus swapchain
  configuration and the per-frame render pass. Currently clears to a flat colour
  and draws nothing.

### Decisions already made, and why

**Isometric will be true 3D under an orthographic camera**, not sorted 2D
sprites. Orthographic projection at 45° yaw and ~35.26° elevation *is* isometric,
and the depth buffer then handles occlusion exactly, in hardware. The sprite
approach would require re-sorting every entity by depth each frame and still
produce popping where entities overlap.

**The horde is drawn with instancing.** One cube mesh, one per-instance buffer
of position/scale/colour, one draw call for all N enemies. Draw call cost is
roughly independent of how much that call draws, so per-entity draw calls are
the failure mode to avoid. (This is why raylib was rejected — its immediate-mode
`DrawCube` forces exactly that.)

**Vsync (`PresentMode::AutoVsync`) is deliberate.** Frame pacing is the
foundation every feel mechanic is measured against: hitstop is "freeze for N
frames", so erratic frame times make identical hits feel different. Changing
present mode needs a stated reason.

### Roadmap

Rough order, one chunk at a time: ortho camera + ground grid + instanced cubes
with runtime-adjustable N and a frame-time HUD → fixed-timestep sim loop with
render interpolation → SoA entity storage → uniform-grid spatial hash for
broadphase → separation steering → attack state machine (startup/active/recovery)
with timed hitboxes → hitstop, knockback, input buffering.
