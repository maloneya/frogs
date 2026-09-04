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

**The rule the layout enforces: `gfx` never knows what an enemy is, and `sim`
never knows what a key is.** Outward, the vocabulary is `Instance` — position,
scale, colour — and `sim` describes itself in it via `extract()`. Inward, the
vocabulary is `Action` — intent — and `app` translates devices into it. Both
dependencies run one way.

```
crates/
  core/  Instance, InstanceBuffer, InstanceSink, MAX_INSTANCES   glam, bytemuck
         Action, ActionMask, InputState, Actions, MoveDir
  gfx/   Renderer, camera, cube, shader.wgsl                     core, wgpu, winit
  sim/   World, Player                                           core, glam  (no wgpu)
  app/   App, Input + BINDINGS, Clock, wiring, main              core, gfx, sim, winit
```

- `core` is the shared vocabulary and belongs to neither side. It deliberately
  does **not** name wgpu — that is what keeps `sim` free of the graphics stack,
  so simulation tests never need a GPU. The vertex layout for `Instance` lives
  in `gfx/cube.rs` for exactly this reason.
- `gfx` — `lib.rs` (surface, device, depth, frame orchestration), `camera.rs`
  (isometric ortho camera, the follow rig, and the uniform), `cube.rs` (mesh,
  pipeline, instance buffer, vertex layout), `shader.wgsl`. The camera rig lives
  here rather than in `app` or `sim` because where the camera points is a
  presentation decision; it is handed a bare `Vec3`, which is exactly as
  anonymous as an `Instance`.
- `sim` — `World`: what exists, plus `step()`, the input/sim seam, and
  `extract()`, the sim/render seam.
- `app` — the wiring layer, and the only crate that sees both sides. `input.rs`
  holds `BINDINGS`, the one place a `KeyCode` becomes an `Action`. GPU state
  is built in `resumed`, not `main`, because winit models surface loss as
  suspend/resume. `about_to_wait` requests a redraw every time the queue drains,
  converting winit's event-driven default into a continuous game loop.
  `time.rs` holds `Clock`; the fixed-timestep accumulator lands in `sim`
  instead, next to the `Dt` it will mint.

### Controls

Game actions (rebindable, go through `Action`):

`WASD` / arrows move the player

Debug commands (fixed, handled straight from the event callback — they act on
the program, not the character, so they deliberately do *not* go through
`Action`):

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
| `sim` cannot name a key or a window | 1 | `crates/sim/build.rs`; `core` never names winit |
| A movement direction is unit-length or zero | 0 | private field; `MoveDir::new` is the only door, and it normalises |
| Movement never leaves the ground plane | 0 | `MoveDir::new` drops the Y component |
| Input edges are consumed exactly once | 0 | the clear lives in `InputState::sample`, the only reader |
| Two keys on one action cannot desync | 0 | `Input.down` tracks *keys*; the action set is derived, never stored |
| The binding table fits its bitset | 1 | `const _: () = assert!(BINDINGS.len() <= u32::BITS …)` |
| A frame's dt cannot teleport the player | 0 | `Clock::tick` clamps what it returns; raw only reaches the HUD |
| Ground + horde + player fit one buffer | 3 | unit test in `sim` at the largest horde the clamp allows |
| The camera basis agrees with the projection | 3 | unit tests in `gfx/camera.rs` |
| Movement speed is frame-rate independent | 3 | unit test in `sim` |
| Camera smoothing is frame-rate independent | 3 | `damp` uses `2^(-dt/half_life)`; unit test in `gfx/camera.rs` |
| The camera target cannot be set unsmoothed | 0 | private field; `follow` is the only writer |
| The camera never overshoots or bobs vertically | 3 | unit tests in `gfx/camera.rs` |
| Turning takes the short way round the ±PI seam | 3 | `shortest_arc` wraps the *difference*; unit test in `sim` |
| Turning is frame-rate independent and never overshoots | 3 | step clamped to the remaining arc; unit tests in `sim` |
| Facing cannot drift toward the precision limit | 3 | `wrap_angle` after every turn; unit test in `sim` |
| Spawning does not swoop the camera in from the origin | 0 | `snap_to`, called in `resumed` before the first frame |

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

**The camera smooths by half-life, not by a per-frame lerp.** `pos.lerp(target,
0.1)` once a frame keeps 90% of the error *per frame* rather than per second:
after one second that is `0.9^60 ≈ 0.002` left at 60Hz but `0.9^144 ≈ 3e-7` at
144Hz — a camera thousands of times tighter purely because the machine is
faster. Here that would be worse than usual, because pressing `V` to uncap the
frame rate would change how the game *feels*, corrupting the measurement `V`
exists to take. `2^(-dt/half_life)` composes exactly under subdivision, so any
number of small steps equals one big one.

**The camera leads the character, and the lead is smoothed separately.** A rigid
offset whips the camera two lead-lengths across the screen the instant you
reverse; a slower half-life on the offset turns that into an ease. The lead is
in world units, not screen ones, so it reveals the same distance in every
direction — the axis threats live on — which is why the vertical lead looks
shorter on screen, foreshortened by sin(35.26°).

### Deliberate choices that look like smells

Do not "clean up" these without understanding why they're there — each one is
load-bearing, and several will compile fine while producing wrong output. Where
a mechanism now enforces one, it is named; the last two are enforced by nothing
but this paragraph, which is precisely why they are worth reading twice.

- **`Instance` carries `yaw` plus two `_pad` floats.** *(enforced: private
  fields + size assert)* Not waste. Vertex buffers have no 16-byte alignment
  requirement so the struct *could* pack to 36 bytes, but the 48-byte stride
  keeps offset maths trivial and reserved room for rotation, hit-flash and team
  id. The reservation has now paid off once: `yaw` moved into the first slot and
  gained the character a facing without touching the vertex layout, the
  attribute array or the `@location` slots. The other two are still spoken for.
  Removing them means rewriting the vertex attribute layout and the shader
  together.
- **Yaw 0 faces `+Z`, positive turns toward `+X`.** *(unenforced — prose in two
  languages)* `Instance::with_yaw`, `World::turn_toward` and `rotate_y` in
  `shader.wgsl` all depend on this one convention, and nothing checks that they
  agree: wgpu validates the *types* crossing into WGSL, never the meaning of the
  numbers. A sign flip would compile, validate, draw, and simply point every
  character 90° off. Closing it properly needs a headless render plus pixel
  readback — which would also be this project's first check that the image is
  *correct* rather than merely accepted.
- **The player is deeper than it is wide** (`0.45 x 1.2 x 0.8`). *(unenforced)*
  A square footprint rotated about the vertical axis looks near-identical at
  every angle, so facing would be real and invisible. The asymmetry is what
  makes the turn readable.
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

- `World::extract()` rebuilds the 16384 static ground tiles every frame and
  re-uploads the entire instance buffer. Still deferred, and now with a number
  behind it: 17409 instances render in 2.81ms uncapped (356fps) on the M4, so
  the static/dynamic buffer split is not yet buying anything.
- `Clock` smooths frame time with an EMA, which *hides* pacing variance. An
  average is the wrong instrument for the thing that matters most here; a
  frame-time histogram is the intended replacement.
- The camera angle is fixed. It tracks and snaps now, but there is still no
  rotation — which is also what lets `ground_basis` be the only screen/world
  translation without a feedback loop between input and view.
- The camera does not clamp to the world bounds, so walking to the very edge
  shows the void. This was measured rather than guessed: the view covers ~57x55
  world units of floor, an axis-aligned footprint ~40 units either side of the
  focus, so on the old 48-unit arena a bounds-clamped camera could have moved
  +/-8 units total — pinned, and following would have stopped working before the
  player reached the edge. Enlarging the world is the fix a camera clamp only
  pretends to be; real level geometry is the eventual one.
- There is no deadzone. Deliberate: it reduces micro-jitter but adds a sticky
  region and a snap at its boundary, and for constant repositioning against a
  horde the smoothed follow reads better. Worth revisiting once combat exists.
- The player passes straight through the horde. Nothing collides with anything
  yet.
- Movement is instantaneous — full speed on the first frame, dead stop on
  release. Deliberate, not an oversight: ARPG movement is essentially instant
  because responsiveness beats momentum, and acceleration is a feel knob better
  tuned against a fixed timestep than a variable one. *Turning* is rate-limited;
  translation is not.
- Only the keyboard is wired. The action layer is what makes a gamepad or
  click-to-move an additive change: a second producer of `ActionMask`, with
  nothing downstream touched.
- Test coverage is uneven: the GPU contract, the sink, the input layer, the
  camera maths and player movement are covered; the cube mesh is not. There is
  still no pixel-level check that the image is *correct*, only that wgpu accepts
  the frame.
- The headless test needs a real adapter, so `cargo test` will not pass in an
  environment without a GPU.

### Roadmap

Rough order, one chunk at a time: ~~ortho camera + ground grid + instanced cubes
with runtime-adjustable N and a frame-time HUD~~ → ~~device input bound to
actions, a player-controlled character that moves and faces where it walks, and
a camera that tracks it~~ → fixed-timestep sim loop with
render interpolation → SoA entity storage → uniform-grid spatial hash for
broadphase → separation steering → attack state machine (startup/active/recovery)
with timed hitboxes → hitstop, knockback, input buffering.

Landing with the fixed-timestep chunk, because they need a sim loop to be
meaningful: `Dt` and `Alpha` as newtypes with private constructors (so a
variable timestep cannot be smuggled in, and interpolation cannot leak into sim
state), `EntityId` from `World::spawn`, a determinism test hashing two identical
runs, and an allocation counter asserting a steady-state frame allocates zero.
