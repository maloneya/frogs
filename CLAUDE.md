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
cargo run                 # debug build, run (default-members points at crates/app)
cargo run --release       # for any performance measurement — debug numbers are meaningless
RUST_LOG=info cargo run   # adapter selection + wgpu diagnostics
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace    # includes a headless GPU test; needs a real adapter
```

Rust was installed via rustup with `--no-modify-path`, so `~/.cargo/bin` is
**not** on PATH by default. Prefix commands with `. "$HOME/.cargo/env" &&`, or
add it to the shell profile.

`[profile.dev.package."*"] opt-level = 3` optimises dependencies while leaving
our crates in debug. Graphics crates are unusably slow otherwise.

## Architecture

Native macOS only (Apple M4 / Metal). Cross-platform and wasm support are
explicit non-goals — a lot of wgpu example code exists to satisfy the browser's
ban on blocking the main thread, and none of that complexity is warranted here.

**The rule the layout enforces: `gfx` never knows what an enemy is.** Its
vocabulary is `Instance` — position, scale, colour. `sim` describes itself in
that vocabulary via `extract()`; the dependency runs one way.

```
crates/
  core/  Instance, InstanceBuffer, InstanceSink, MAX_INSTANCES   glam, bytemuck
  gfx/   Renderer, camera, cube, shader.wgsl                     core, wgpu, winit
  sim/   World                                                   core, glam  (no wgpu)
  app/   App, Clock, wiring, main                                core, gfx, sim, winit
```

- `core` is the shared vocabulary and belongs to neither side. It deliberately
  does **not** name wgpu — that is what keeps `sim` free of the graphics stack,
  so simulation tests never need a GPU. The vertex layout for `Instance` lives
  in `gfx/cube.rs` for exactly this reason.
- `gfx` — `lib.rs` (surface, device, depth, frame orchestration), `camera.rs`
  (isometric ortho camera + uniform), `cube.rs` (mesh, pipeline, instance
  buffer, vertex layout), `shader.wgsl`.
- `sim` — `World`: what exists, plus `extract()`, the sim/render seam.
- `app` — the wiring layer, and the only crate that sees both sides. GPU state
  is built in `resumed`, not `main`, because winit models surface loss as
  suspend/resume. `about_to_wait` requests a redraw every time the queue drains,
  converting winit's event-driven default into a continuous game loop.
  `time.rs` holds `Clock`; the fixed-timestep accumulator lands in `sim`
  instead, next to the `Dt` it will mint.

### Controls

`[` / `]` halve and double N · `-` / `=` zoom · `V` toggle vsync · `Esc` quit

## Structural invariants, and what actually enforces them

The organising idea: **an invariant belongs to the type that owns the data,
enforced at the only door into it** — not to the caller that happens to write it
today. Prose in this file is the weakest form of enforcement, because it is read
at session start and then not again while editing. Prefer, in order:

| | Layer | Mechanism | Can it be quietly bypassed? |
|---|---|---|---|
| 0 | Unrepresentable | crate graph, module privacy, private fields | No — needs a visible `pub`/manifest diff |
| 1 | Won't compile | newtypes, `[workspace.lints]`, const asserts, `build.rs` guards | Only via a loud `#[expect(reason = "…")]` |
| 2 | Won't validate | wgpu pipeline validation, headless | No — it is the driver's rule, not ours |
| 3 | Won't pass | unit tests | Yes, by editing the test |
| 4 | Won't go unnoticed | this file | Yes |

**When adding an invariant, put it as high up that table as it will go, and say
why if it cannot go higher.** What is in place today:

| Invariant | Layer | Mechanism |
|---|---|---|
| `gfx` cannot name a simulation type | 0 | separate crates — `use arpg_sim::…` is E0432 |
| `gfx` cannot depend on `sim`; `sim` cannot depend on wgpu/winit | 1 | `crates/{gfx,sim}/build.rs`, run on every build |
| Enemy count stays within the instance budget | 0 | private field; `World::set_enemy_count` clamps |
| Zoom stays in a sane range | 0 | private field; `OrthoCamera::zoom_by` clamps |
| Aspect ratio survives a minimised window | 0 | `aspect_of` guards inside the camera |
| `Renderer.vsync` cannot desync from the surface | 0 | private field; `toggle_vsync` is the only writer |
| `Instance` padding is never written | 0 | private fields; `Instance::new` is the only door |
| No allocation or overflow at the extract seam | 0 | `InstanceSink` exposes `push` and nothing else |
| The buffer is reset once per frame | 0 | reset lives in `InstanceBuffer::sink()` |
| `Instance` is exactly 48 bytes | 1 | `const _: () = assert!(…)` beside the type |
| Rust vertex layout matches `shader.wgsl` | 2 | headless pipeline + draw test in `gfx/src/lib.rs` |
| Public API stays deliberate | 1 | `unreachable_pub = "deny"` |
| The sink's cap holds at its real value | 3 | unit tests in `core` |

**Escape hatch:** `#[expect(lint, reason = "…")]`, never `#[allow]`. `expect`
stops compiling once the violation it covers disappears, so suppressions cannot
go stale unnoticed and each one carries a written reason.

One correction worth recording, because the reasoning is tempting and wrong:
splitting into crates does **not** make `gfx → sim` a Cargo cycle. They are
siblings, both depending only on `core`, so Cargo accepts that edge without
complaint. The `build.rs` guards exist precisely because the cycle argument
does not hold.

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

**Vsync (`PresentMode::AutoVsync`) is the default, with a toggle.** Frame pacing
is the foundation every feel mechanic is measured against: hitstop is "freeze for
N frames", so erratic frame times make identical hits feel different.

But vsync *quantises* frame time to multiples of the refresh interval — under it
a 4ms renderer and a 16ms renderer look identical, and cost appears as a cliff to
33.3ms rather than a climb. `V` switches to `Immediate` (supported on this
Metal surface) for measurement. Measure uncapped; tune feel under vsync.

**Colours are specified in linear space.** The surface is `Bgra8UnormSrgb`, so
the hardware encodes on write. Passing the sRGB value you want yields something
roughly five times too bright.

### Deliberate choices that look like smells

Do not "clean up" these without understanding why they're there — each one is
load-bearing, and several will compile fine while producing wrong output. Where
a mechanism now enforces one, it is named; the last two are enforced by nothing
but this paragraph, which is precisely why they are worth reading twice.

- **`Instance` has three `_pad` floats.** *(enforced: private fields + size
  assert)* Not waste. Vertex buffers have no 16-byte alignment requirement so the
  struct *could* pack to 36 bytes, but the 48-byte stride keeps offset maths
  trivial and reserves room for rotation, hit-flash and team id. Removing the
  padding means rewriting the vertex attribute layout and the shader together.
- **The instance buffer is allocated at full `MAX_INSTANCES` capacity** and only
  partially written. *(enforced: `InstanceSink`)* Capacity and count are separate
  on purpose — regrowing a GPU buffer mid-run means syncing against in-flight
  frames. The CPU-side `InstanceBuffer` preallocates to match.
- **The cube has 24 vertices, not 8.** *(unenforced — no test yet)* Each face
  needs its own normal and a vertex carries one. Deduplicating to 8 corners
  silently destroys the shading.
- **Cube winding is derived from a per-face orthonormal basis**, not written out
  as a literal table. *(unenforced — no test yet)* That's what guarantees correct
  outward winding under back-face culling; a hand-written table is where
  inside-out faces come from.
- **Colour literals look far too dark.** *(unenforced; a `LinearRgb` newtype
  would fix this and is worth doing)* They're linear; the surface is sRGB and the
  hardware encodes on write. `0.05` on screen is `0.0039` in source.
- **`about_to_wait` requests a redraw unconditionally.** *(prose only — not
  mechanisable)* This is what makes the loop continuous rather than
  event-driven. It is not a busy-wait bug.
- **Depth uses `StoreOp::Discard`.** *(prose only — not mechanisable)* Nothing
  reads depth after the pass; storing it would waste real bandwidth on a tiled
  GPU.

### Known limitations (real, not yet worth fixing)

- `World::extract()` rebuilds the 1024 static ground tiles every frame and
  re-uploads the entire instance buffer. The fix is separate static/dynamic
  buffer regions — deliberately deferred until measurement shows it costing
  something.
- `Clock` smooths frame time with an EMA, which *hides* pacing variance. An
  average is the wrong instrument for the thing that matters most here; a
  frame-time histogram is the intended replacement.
- The camera angle is fixed. No rotation, and no way to look at anything but
  the origin.
- Test coverage is thin: the GPU contract and the sink are covered, the mesh and
  the camera maths are not. There is still no pixel-level check that the image
  is *correct*, only that wgpu accepts the frame.
- The headless test needs a real adapter, so `cargo test` will not pass in an
  environment without a GPU.

### Roadmap

Rough order, one chunk at a time: ~~ortho camera + ground grid + instanced cubes
with runtime-adjustable N and a frame-time HUD~~ → fixed-timestep sim loop with
render interpolation → SoA entity storage → uniform-grid spatial hash for
broadphase → separation steering → attack state machine (startup/active/recovery)
with timed hitboxes → hitstop, knockback, input buffering.

Landing with the fixed-timestep chunk, because they need a sim loop to be
meaningful: `Dt` and `Alpha` as newtypes with private constructors (so a
variable timestep cannot be smuggled in, and interpolation cannot leak into sim
state), `EntityId` from `World::spawn`, a determinism test hashing two identical
runs, and an allocation counter asserting a steady-state frame allocates zero.
