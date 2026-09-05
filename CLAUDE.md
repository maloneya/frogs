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

## How work is finished here

Five streams shape every change here. The rationale, current state and open
work for each is in [`docs/agent-principles.md`](docs/agent-principles.md);
what follows are the rules that apply *while editing*.

1. **Sim layer** — `sim` is a pure function of (state, inputs). No wall clock,
   no unseeded randomness, no iteration over hash-ordered containers. The
   invariant binds simulation only: presentation (interpolation, smoothing,
   particles, audio) is exempt, and must never feed back into sim state.
2. **Hooks** — a new behaviour is a named pass in the sim schedule, taking the
   slices it declares. Do not add one by editing the body of `World::step`.
   See the `add-sim-pass` skill.
3. **Perception** — anything an agent must observe is a trace event or a
   *derived* `state` field. Never a hand-maintained format string.
4. **Scenarios** — a change to sim behaviour is not done until a scenario
   asserts it and the scenario runner exits 0. See the `scenario` skill.
5. **Traps** — before diagnosing a symptom, grep
   [`docs/traps.md`](docs/traps.md). After losing more than ten minutes to
   one, append an entry.

Rule 4 is the stopping condition, not a suggestion. **A change verified by
reading numbers off `state` and judging them correct yourself is unfinished
work** — that is the agent grading its own homework, and it is the one failure
mode none of the machinery above can catch. Predicting a value and then
asserting it in a scenario is the same act, made durable and checkable.

The `Stop` hook in `.claude/settings.json` enforces this: it runs the test
suite **and the scenario runner** when a turn ends, and blocks on failure. Both
halves are live as of roadmap chunk 3, so rule 4 is now enforced by a process
exiting nonzero rather than by this paragraph.

## Commands

```sh
cargo run                 # debug build, run (default-members points at crates/app)
cargo run --release       # for any performance measurement — debug numbers are meaningless
RUST_LOG=info cargo run   # adapter selection + wgpu diagnostics
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace    # includes a headless GPU test; needs a real adapter

cargo run --quiet -p scenario -- scenarios/   # the gate: exits 0 or 1, no GPU, no window
```

### Driving the game from a shell

`ARPG_HARNESS` opens a unix socket that plays the game: real keys through the
real binding table, screenshots the app takes of itself, and simulation state as
numbers. Unset, none of it exists — no socket, no thread, no way in.

```sh
ARPG_HARNESS=/tmp/arpg.sock cargo run --release
echo 'hold d 500' | nc -U /tmp/arpg.sock     # walk east for 500ms, replies when done
echo 'shot /tmp/f.png' | nc -U /tmp/arpg.sock # replies once the file exists
echo state | nc -U /tmp/arpg.sock
```

`press`/`release`/`tap`/`hold <key> <ms>` · `wait <ms>` · `shot <path>` ·
`state` · `enemies <n>` · `vsync on|off` · `quit`

Key names are a column of `BINDINGS` rather than a table beside the harness, so
binding a key makes it drivable in the same edit.

Every command replies, and the reply means the effect has *landed* — `hold`
answers after the key comes back up, `shot` after the file is on disk. So a test
is a sequence of commands, not a sequence of sleeps and hopes.

Why it exists: driving the game through the OS instead (synthetic keystrokes
plus a desktop screenshot tool) needs the window frontmost, the display awake,
and accessibility permission, and when any of those is false it does not fail —
the keys go to whatever *is* focused and the screenshot comes back black. Both
look exactly like the game being broken. Building this cost less than the time
already lost to a stray keypress silently resizing the horde mid-test.

**Reading perf from it:** compare `frames` across a `wait`, rather than trusting
`frame_ms`, which is an EMA and cannot tell a steady 60Hz from a mix averaging
to it. Then sanity-check against `skipped` *and* against the other present mode.
An occluded window hands back no texture, the draw is skipped, and a loop
spinning on nothing reports thousands of frames a second; a merely *backgrounded*
window presents honestly but throttled, with `skipped` at zero. Uncapped that is
not several times vsync means the app is throttled, not that the renderer is
slow.

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
         Action, ActionMask, InputState, Actions, MoveDir, damp
  gfx/   Renderer, camera, cube, capture, shader.wgsl            core, wgpu, winit, png
  sim/   World, Player, Dt/Alpha/Accumulator, hash               core, glam  (no wgpu)
  app/   App, Input + BINDINGS, Clock, harness, wiring, main     core, gfx, sim, winit
  scenario/  the headless gate: run a .ron, assert, exit 0/1     core, sim, ron  (no gfx)
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
  `time.rs` holds `Clock`, which now only *measures*: the fixed-timestep
  accumulator lives in `sim`, next to the `Dt` it mints, so the crate that can
  read a clock cannot turn what it reads into simulation time.

### Controls

Game actions (rebindable, go through `Action`):

`WASD` / arrows move the player

Debug commands (fixed, handled straight from the event callback — they act on
the program, not the character, so they deliberately do *not* go through
`Action`):

`[` / `]` halve and double N · `-` / `=` zoom · `V` toggle vsync ·
`P` screenshot (to `$ARPG_CAPTURE_DIR`, default the temp dir) · `Esc` quit

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

The inventory of what is enforced today lives in
[`docs/invariants.md`](docs/invariants.md). It is deliberately *not* here: it is
long, it duplicates enforcement that already exists in the code, and a second
table is a table that can silently disagree with the first — the exact failure
`BINDINGS` was restructured to avoid. Read it when auditing; do not treat it as
authoritative over the code.

The lint wall is applied at write time, not just at build time: the
`PostToolUse` hook in `.claude/settings.json` runs
`cargo clippy --workspace --all-targets -- -D warnings` after every file-editing
tool **and after every Bash call**, and blocks on failure. The Bash matcher is
load-bearing rather than belt-and-braces — an agent editing through a shell
heredoc produces no `file_path`, so a hook keyed only on Edit/Write never fires
and the gate silently degrades to "remember to run clippy", which this file's
own table rates as the weakest layer there is.

**Escape hatch:** `#[expect(lint, reason = "…")]`, never `#[allow]`. `expect`
stops compiling once the violation it covers disappears, so suppressions cannot
go stale unnoticed and each one carries a written reason.

One correction worth recording, because the reasoning is tempting and wrong:
splitting into crates does **not** make `gfx → sim` a Cargo cycle. They are
siblings, both depending only on `core`, so Cargo accepts that edge without
complaint. The `build.rs` guards exist precisely because the cycle argument
does not hold.

Those guards are **allowlists**, and the reason is worth keeping. They began as
denylists naming `arpg-gfx`, `wgpu` and `winit` — which caught exactly the three
mistakes someone had already imagined, and let `bevy`, `hecs` and `rapier` walk
straight into `sim`, against the loudest rule the project has. A denylist fails
open; you have to predict the mistake. An allowlist fails closed. Widening one is
a deliberate, visible edit to a file whose whole job is saying what may be
depended on.

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
- **Yaw 0 faces `+Z`, positive turns toward `+X`.** *(enforced: pixel test)*
  `Instance::with_yaw`, `World::turn_toward` and `rotate_y` in `shader.wgsl` all
  depend on this one convention, and wgpu cannot check it — it validates the
  *types* crossing into WGSL, never the meaning of the numbers, so a sign flip
  compiles, validates, draws, and points every character 90° off in silence.
  `yaw_points_the_body_where_the_convention_says` renders a long bar headless and
  reads the pixels back. Note its second half: a bounding box is
  **reflection-invariant**, so swapping `sin` and `cos` — which mirrors every
  direction across the screen vertical — passed a box-only version of this test.
  Measuring which way the bar *leans* is what catches it. Verified by mutation:
  sign flip, dropped rotation, swapped sin/cos, negated yaw, and scale-after-
  rotation are all caught.
- **The player is deeper than it is wide** (`0.45 x 1.2 x 0.8`). *(enforced:
  const assert)* A square footprint rotated about the vertical axis looks
  near-identical at every angle, so facing would be real and invisible. The
  asymmetry is what makes the turn readable, so it is a compile error to remove
  it rather than a note somebody might read.
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

### Known limitations, and the roadmap

Both moved to [`docs/roadmap.md`](docs/roadmap.md), which now carries an exit-code
gate per chunk. They lived here, were read once at session start, and grew; the
roadmap is consulted when choosing work rather than while editing, so it belongs
in a file loaded on demand.
