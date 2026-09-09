# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

A from-scratch action-RPG engine, built as a **learning project**. The goal is
understanding engine-level technology, not shipping a game. Specifically: making
combat against a large horde of enemies *feel* right.

This framing changes how to work here:

- **Simulation and systems first.** Build behaviour by composing systems.
  Shared physical interaction belongs below attacks and other producers.
- **Agent-first engine.** A system must be drivable, observable and assertable
  through the engine's own interfaces, independently of the feature using it.
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
halves are live, so rule 4 is enforced by a process exiting nonzero rather than
by this paragraph.

### Engine and game, and where the line falls

**The test is not reuse.** This engine is not trying to be general — it exists
to make one game, and "could another game use this?" would push `contact.rs`
and `pass/separate.rs` toward configurability nothing needs.

The test that matters is blast radius: **the engine is whatever holds an
invariant that could be violated silently; the game is tunable content no
invariant guards.** [`docs/invariants.md`](docs/invariants.md) is the manifest
of the first half.

So the line runs *through* files rather than between them.
`crates/sim/src/pass/seek.rs` is engine: it carries the assert that a chaser
must be slower than the player, without which kiting stops existing. The `3.5`
that assert bounds is game. Same in `pass/attack.rs` — that startup, active and
recovery are each non-zero is engine; `6`, `4` and `10` are game.

The rule that follows, and the one to apply while editing: **the game
vocabulary has exactly one definition, and it lives in `sim` beside the thing
it describes.** `Template`, `Condition`, `Placement`, `Source` and `SourceSpec`
are that vocabulary; the scenario `.ron` format and the `ARPG_HARNESS` command
parser *derive* from those types rather than restating them. A second
hand-written copy of an axis is the failure this rule exists to prevent, and it
fails in the direction nothing reports — not by breaking a build, but by
leaving a new capability unreachable from outside.

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
real binding table, screenshots the app takes of itself, simulation state as
JSON, and the event trace. Unset, none of it exists — no socket, no thread, no
way in.

```sh
ARPG_HARNESS=/tmp/arpg.sock cargo run --release
echo 'hold d 500' | nc -U /tmp/arpg.sock
```

`press`/`release`/`tap`/`hold <key> <ms>` · `wait <ms>` · `shot <path>` ·
`state` · `trace since <tick>` · `enemies <n>` · `seekers <n>` ·
`spawn <x> <z> [seek]` ·
`source <x> <z> [seek] [every <n>] [ring <r>] [near <r>] [fewer <n>]` ·
`source remove <id>` · `impulse <player|#id> <x> <z>` · `vsync on|off` · `quit`

Scene playtests: `scene start <path>` starts fresh from a file; `scene restart`
repeats the selected snapshot; `scene add <path>` / `scene evict <id>` exercise
additive content; `scene list` reports live instances. See
[`docs/scene-playtests.md`](docs/scene-playtests.md) for completion semantics,
run-scoped identities and the shared scenario-file path.

Every command replies, and the reply means the effect has **landed** — `hold`
answers after the key comes back up, `shot` after the file is on disk. So a test
is a sequence of commands, not a sequence of sleeps and hopes. Key names are a
column of `BINDINGS`, so binding a key makes it drivable in the same edit.

**The `playtest` skill is the rest of this** — measuring throughput without
being lied to, what a screenshot needs, and why driving the game through the OS
instead fails silently. Do not reproduce it here.

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
scale, colour — and `sim` describes itself in it via `extract()`. Inward, a
device becomes an `Action`, named in *screen* directions; `app` asks the camera
to resolve those to world space and hands `sim` an `Intent`. Both dependencies
run one way, and the simulation sees neither a key nor a screen.

The overlay is that same rule applied one layer up. `Quad` is a rectangle in
pixels with a patch of atlas and a colour, and `app` describes a screen in it
via `hud::draw` — so the renderer draws a health bar without learning what
health is, exactly as it draws an enemy without learning what an enemy is.

`Quad` lives in `gfx` rather than `core`, and the difference from `Instance` is
the point: `core`'s bar is *needed by both, beholden to neither*, and `Instance`
meets it because `sim` must mint one without linking wgpu. Nothing that has to
stay ignorant of the graphics stack ever mints a quad. Keeping it in `gfx` is
what lets `Quad::textured` be `pub(crate)` and lets the atlas's reserved white
texel *derive* the UV that names it, instead of two crates agreeing by
convention across a boundary no compiler spans.

```
crates/
  core/  Instance, InstanceBuffer, InstanceSink, MAX_INSTANCES   glam, bytemuck
         Action, ActionMask, InputState, Actions            (input.rs)
         MoveDir, Intent                                    (intent.rs)
         Report, damp
  gfx/   Renderer, camera, cube, capture, shader.wgsl      core, wgpu, winit, png,
         Quad/QuadBuffer/QuadSink, Glyphs, overlay.wgsl                  fontdue
  sim/   World, pass/ schedule, Dt/Alpha/Accumulator, trace   core, glam, serde
  app/   App, Controls + BINDINGS, Clock, harness, hud, ui      core, gfx, sim, winit
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
- `sim` — `World`: what exists. `step()` is the input/sim seam and is nothing
  but an ordered list of calls into `pass/`, one module per named pass, each
  owning its tuning constants and taking the data it declares rather than
  `&mut World`. `extract()` is the sim/render seam, `trace()` the sim/agent one;
  both hand out shared references, which is what makes "perception cannot change
  what it observes" a fact about the types. Its own modules:
  - `slots.rs` — `EntityId` and the map from a name to a dense row. The horde's
    arrays are contiguous, so a despawn moves rows; only a generational id
    survives that.
  - `members.rs` — which entities have a given behaviour. A behaviour is its own
    membership plus a pass that walks it, so adding one touches no existing type
    and costs what it uses rather than what the horde costs.
  - `contact.rs` — whether two bodies touch, and along what line. It stops
    there, because separation, a hitbox and a trigger are three answers to that
    one question.
  - `pass/` — one module per named pass: `source`, `spawn`, `remember`, `walk`,
    `seek`, `motion::integrate`, `separate`, `contain`, `motion::settle`,
    `face`, `attack`, in that order. The first
    two are *decide* and *perform*, split on purpose: `source` works out which
    sources fire and may only push onto the spawn queue — it is handed no
    storage, so the code that decides new bodies exist cannot make one — and
    `spawn` is the only pass that changes *what exists*. The horde's length is
    then constant for the rest of the tick, so no other pass has to defend
    against a row moving underneath it.
  - `source.rs` — who asks for spawns, and when. A source is **not** a kind of
    body: it is what makes bodies, so hanging it off one of its own products
    inverts the layering and breaks as soon as the thing made is not a body. It
    is four independent axes — cadence, condition, placement, template — rather
    than one enum of every useful combination. `pass/mod.rs` is the readable
    copy of the schedule and must agree with the body of `World::step`. The
    hitbox is `contact`'s question with a different answer, and the signatures
    say so: the solver takes `&mut [Vec2]`, the hitbox `&[Vec2]` plus an `ImpulseSink`, which cannot write positions.
  - `pass/motion.rs` — sparse physical membership, mass, carried velocity and
    validated impulses. Collision response exchanges momentum; damping settles
    it. Powered walk/seek displacement remains independent. The player shares
    body storage with the horde; its row survives enemy resets and despawns.
- The **overlay** is the second pipeline, and almost the inverse of the first:
  no camera, alpha blended, drawn in submission order rather than depth order.
  It joins the world's render pass instead of opening its own — declaring a
  depth state to match, then never writing depth and always passing the test —
  because a second pass would store and reload the whole frame on a tiled GPU
  for nothing. `gfx/text.rs` rasterises printable ASCII into one atlas with
  `fontdue` and keeps the metrics; `gfx/quad.rs` draws it. Texel `(0, 0)` of
  that atlas is reserved opaque white, which is what lets `Quad::solid` share
  the pipeline: a bar is a quad whose UVs collapse onto it. One draw call for
  text and flat colour together, forever.
  - `Glyphs` is the metrics table **without** the texture, and the split is what
    makes an overlay's layout assertable with no GPU: `hud::draw` is a pure
    function, so "what would be on screen" is a list of rectangles a test reads.
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

`WASD` / arrows move the player · `space` swings

`F1` opens the attack profile panel · arrows choose Basic, Thrust, Sweep, or
Heavy sweep · `R` returns to Basic ·
`F1` / `Esc` close. The panel captures gameplay input while the world keeps
running. Selections apply to the next swing and last for the current run.

`F2` opens the scene picker · Up/Down select · Enter starts fresh · F2/Esc
close. Restart current reuses its cached snapshot; file choices reread disk.
See [`docs/scene-playtests.md`](docs/scene-playtests.md).

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

The lint wall applies at **write** time, not just at build time: the
`PostToolUse` hook in `.claude/settings.json` runs clippy after every
file-editing tool **and after every Bash call**, and blocks on failure. The Bash
matcher is load-bearing rather than belt-and-braces — an agent editing through a
shell heredoc produces no `file_path`, so a hook keyed only on Edit/Write never
fires and the gate degrades to "remember to run clippy", which the table above
rates as the weakest layer there is.

**Escape hatch:** `#[expect(lint, reason = "…")]`, never `#[allow]`. `expect`
stops compiling once the violation it covers disappears, so a suppression cannot
go stale unnoticed and each one carries a written reason.

**Crate dependencies are allowlists**, in `crates/{gfx,sim,scenario}/build.rs`,
and they fail closed. Two things about them are tempting to get wrong, so both
are argued at the definition site: splitting into crates does *not* make
`gfx → sim` a Cargo cycle — they are siblings, and Cargo accepts that edge — so
the guards are doing work nothing else does. And a denylist would fail open:
naming `wgpu` and `winit` catches the mistakes someone already imagined and lets
`bevy` walk straight in.

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

**The camera smooths by half-life, not by a per-frame lerp.** A per-frame lerp
keeps 90% of the error *per frame* rather than per second, so the camera is
thousands of times tighter at 144Hz than at 60Hz — and pressing `V` would then
change how the game feels, corrupting the measurement `V` exists to take.
`2^(-dt/half_life)` composes exactly under subdivision. The arithmetic is in
`core::damp` and `gfx/camera.rs`.

**The camera leads the character, and the lead is smoothed separately.** A rigid
offset whips the camera two lead-lengths across the screen the instant you
reverse. The lead is in world units rather than screen ones, so it reveals the
same distance in every direction.

### Deliberate choices that look like smells

Do not "clean up" these without understanding why they are there — each is
load-bearing, and several compile fine while producing wrong output.

**Where a mechanism enforces one, only the mechanism is named here**: the
argument lives at the definition site, which is where someone about to change it
is already looking. The unenforced ones keep their full reasoning, because for
those this paragraph *is* the enforcement — which is exactly why they are the
ones worth reading twice.

Enforced, so this is a pointer and not an argument:

- **`Instance` carries `yaw` plus two `_pad` floats.** Reserved headroom, not
  waste. *(private fields + size assert; see `core/instance.rs`)*
- **Yaw 0 faces `+Z`, positive turns toward `+X`.** A sign flip compiles,
  validates, draws, and points every character 90° off in silence. *(pixel
  test in `gfx/src/lib.rs`, mutation-checked)*
- **The player is deeper than it is wide** (`0.45 x 1.2 x 0.8`), or its facing
  would be real and invisible. *(const assert)*
- **The instance buffer is allocated at full `MAX_INSTANCES`** and only
  partially written. *(`InstanceSink`)*

Enforced by nothing but this list:

- **The cube has 24 vertices, not 8.** Each face needs its own normal and a
  vertex carries one. Deduplicating to 8 corners silently destroys the shading.
- **Cube winding is derived from a per-face orthonormal basis**, not written out
  as a literal table. That is what guarantees correct outward winding under
  back-face culling; a hand-written table is where inside-out faces come from.
- **Colour literals look far too dark.** They are linear; the surface is sRGB
  and the hardware encodes on write. `0.05` on screen is `0.0039` in source. A
  `LinearRgb` newtype would move this up the ladder and is worth doing.
- **`about_to_wait` requests a redraw unconditionally.** This is what makes the
  loop continuous rather than event-driven. It is not a busy-wait bug.
- **Depth uses `StoreOp::Discard`.** Nothing reads depth after the pass; storing
  it would waste real bandwidth on a tiled GPU.
