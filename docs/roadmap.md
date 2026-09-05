# Roadmap

One chunk at a time, built and running before the next starts. Each chunk names
the agent-principles stream it advances (see `docs/agent-principles.md`) and
carries a **gate**: the thing that must exit 0 before the chunk is done. "It
looks right when I run it" is not a gate.

Chunks 1–3 change what every later chunk costs, which is why they come first
even though none of them is a game feature.

## Done

- ~~ortho camera + ground grid + instanced cubes, runtime-adjustable N, frame-time HUD~~
- ~~device input bound to actions, a player-controlled character that moves and
  faces where it walks, and a camera that tracks it~~

## 1a. Fixed timestep — *sim layer* — **done**

`Dt` is a unit struct with a private field, minted only by `sim::Accumulator`.
It carries no number at all, which is a stronger claim than a newtype around
`f32`: a wrong duration is unrepresentable rather than merely hard to build.
`World` counts ticks and hashes all of its state, exhaustively by
destructuring, so a field added without being hashed is E0027.

The `MAX_FRAME_TIME` clamp moved out of `app`'s `Clock` and became
`MAX_TICKS_PER_FRAME` in the accumulator — the same guard, expressed in the unit
that decides it, next to the discard that stops a stall being repaid.

**Gate: met.** `one_input_stream_replays_to_the_same_hash_every_tick` compares
per-tick hash sequences across a ragged frame schedule;
`frame_rate_cannot_change_the_simulation` compares 1, 2 and 4 ticks per frame;
`a_steady_state_frame_allocates_nothing` runs 60 sim+extract frames under a
thread-local counting allocator. Eight mutations were checked against these,
and two of them escaped the first version — see the notes on
`every_field_of_the_world_reaches_the_hash` and
`walking_covers_the_speed_it_claims` / `turning_covers_the_rate_it_claims`, both
of which exist because of that.

## 1b. Render interpolation — *sim layer* — **done**

`Alpha` from `Accumulator::alpha()`, previous positions kept beside current
ones, the blend applied at `extract()`. Presentation only, enforced at layer 0
rather than by a rule: `extract` takes `&self`, so there is no `&mut` for a
blended value to be written back through.

The trade, written down because it is a real cost and not an oversight:
interpolating between the last two ticks puts the image a *constant* one tick
(16.7ms) behind the simulation, where drawing the latest tick directly is
between 0 and 16.7ms behind. Constant latency is the better deal here — hands
adapt to a fixed offset within minutes, and variable pacing is exactly what
makes two identical hits feel different, which is the thing this project exists
to measure.

Facing blends the short way round the ±PI seam. A naive lerp there spins the
body 6.26 radians the wrong way for a single frame, which reads as a flicker and
gets blamed on the renderer.

**Gate: met.** `the_blend_endpoints_are_the_two_ticks_themselves`,
`the_blend_crosses_the_gap_once_and_in_order`, `the_horde_is_interpolated_too`,
`drawing_never_touches_sim_state`,
`the_drawn_facing_crosses_the_pi_seam_the_short_way`,
`the_previous_tick_is_the_previous_tick` and
`a_respawned_horde_is_drawn_standing_still`.

Eight mutations checked, of which **four escaped the first version**: three
because every blend test read the player (the last instance, and so the easy
one) while the horde — a thousand of the bodies on screen — went unchecked, and
one because every test stepped immediately after a respawn, which hides a stale
`prev`. The lesson is worth keeping: a test written against the convenient
entity is not a test of the system.

### (ordered after chunk 3 — see the note there)

Tick-stamped events in a 16k ring buffer, `trace since <tick>` on the harness
socket, and golden trace files in scenarios. Taken together with the pass split
from chunk 4, because a `TraceSink` is one of the things a pass declares and
doing them apart would have meant doing the split twice.

`TraceSink` is bound to the tick being run, so **a pass cannot stamp an event
with the wrong tick** — the one error that would make every timing assertion
built on this worthless. It mirrors `InstanceSink`, the other seam out of `sim`.

Events summarise per tick (`contacts count=37`, not thirty-seven events);
per-occurrence events are reserved for things that occur rarely, which is what
hitboxes and hits will be. The trace is deliberately **not** hashed, and the
exhaustive destructuring in `World::hash` is what forced that to be a written
decision rather than an omission.

**Gate: met.** `shoving_through_the_horde.ron` carries a checked-in golden
trace. Mutation-checked: a contact radius moved by 0.02 adds one line, and
**reordering two passes** — separation before walking instead of after — changes
the contact count on tick 1. Moving `remember` out of first place is caught
instead by three unit tests, which is the honest split: the trace sees
simulation ordering, and `remember` exists for *interpolation*, which is not
scenario-observable until offscreen capture in chunk 6.

The trace earned its keep on the first golden file it produced. Tick 3 is
missing from it — the player bounces clear of the crowd for exactly one tick —
which no final-state assertion could ever show.

Lands *before* the first hitbox rather than after. Attack windows, hitstop and
knockback fail without a crash or a compiler error, and a state snapshot taken
afterward cannot see a mistimed frame inside an active window.

**Gate:** movement and contact events appear in a checked-in golden trace, and a
deliberate off-by-one in a pass changes that file.

## 3. Scenario runner — *scenarios* — **done**

`crates/scenario`, allowlisted to `core` + `sim` + `glam` + `ron`/`serde`. RON
setup, tick-indexed input spans, assertions over final state, a tick budget,
exit 0 or 1. **The `Stop` hook's dormant half is now live**, so rule 4 in
`CLAUDE.md` is enforced by a process rather than by a paragraph.

Taken before chunk 2, deliberately reversing the original order. The argument:
the runner is the *stopping condition* for the other four streams, everything it
needed already existed after chunk 1, and nothing it needed was waiting on the
trace. Building it first means the trace lands in chunk 2 with a consumer that
already asserts on it, rather than as a feature nobody checks.

Two decisions worth knowing before writing a scenario:

- **Directions are world space, not screen space.** The screen mapping belongs
  to the camera because it depends on the camera's angle, which is presentation
  and may change. A scenario that spoke screen space would have to be rewritten
  the day the view rotates.
- **Every scenario is also a replay test**, run twice and compared by per-tick
  hash whether or not it asks. Free, and it is the only way a property that
  subtle stays checked.

`set_enemy_count` now accepts zero. That was wrong on its own terms — enemies
are going to die, and an empty arena is a state reached by playing well — and it
was also what made the simplest possible scenario impossible: any horde at all
starts in contact with the player, so every prediction about movement was really
a prediction about the solver.

**Gate: met.** `cargo run -p scenario -- scenarios/` exits 0 with no GPU and no
window; five scenarios pass. Verified by mutation, all caught with the exit code
and a message naming the cause: `PLAYER_SPEED` drifting 9.0→9.05; the wall
clamping the centre instead of the body edge; a wall clock inside `step`
(reported as "diverged at tick 3"); and an off-by-one in the runner's *own*
input scheduling (reported as "off by 0.1500 — that is 1.00 ticks of walking").

Still true, and still owed: GPU-dependent tests are not yet behind
`#[cfg(feature = "gpu")]`, so `cargo test --workspace` still needs an adapter.
The scenario runner does not.

## 2. Trace stream — *perception* — **done**, with 4's pass split

## 4. SoA entity storage — *hooks*

**The pass-decomposition half is done** (with chunk 2). `step` is an ordered
list of named calls and nothing else; `pass/mod.rs` documents the order and why
each adjacency is what it is; each pass owns its tuning constants, their const
asserts, and its tests. Constants split on a rule worth keeping: a constant
describing an *entity* (radius, scale, spacing) lives with the storage, and one
describing a *behaviour* (speed, turn rate, mass ratio) lives with the pass.

Doing it before the trace rather than after would have meant doing it twice, and
doing it now rather than later was only *safe* because chunk 1's per-tick hash
made a behaviour-preserving refactor checkable — the six scenarios passing
unchanged is what proved the split changed nothing.

**Still open:** `EntityId` from `World::spawn`, the player joining the horde's
storage so passes take slices for it too, and `spawn`/`place` as scenario setup
primitives. That last one is the binding constraint on scenarios today: the only
setup available is a horde count, so anything needing a body in a specific place
cannot be written yet.

**Gate:** scenarios stay green across the refactor; a scenario asserts entity
identity survives despawn and reuse.

## 5. Uniform-grid spatial hash for broadphase — *sim layer*

**Gate:** brute-force and grid broadphase produce identical contact sets over a
replayed input stream. A headless perf assertion enters the scenario runner
here: `step()` at 4096 enemies stays under its budget, measured without a GPU or
a visible window.

## 6. Offscreen capture — *perception*

Render to a target rather than the swapchain, so screenshots stop depending on
window visibility and become reproducible tick-for-tick.

**Gate:** the yaw pixel test and a `shot` scenario both pass with no window
present.

## 7. Separation steering — *sim layer*

**Gate:** a scenario spawning a deliberately overlapped cluster asserts it
resolves without explosion, within a tick budget, with no NaN.

## 8. Attack state machine — startup / active / recovery, timed hitboxes

The payoff chunk for streams 2–4, and the first one where the machinery is doing
work no unit test could.

**Gate:** scenarios asserting the exact ticks a hitbox activates and
deactivates, and that a hit landing one tick outside the window does not
register.

## 9. Hitstop, knockback, input buffering

**Gate:** golden traces for impulse magnitude and hitstop duration, so a tuning
change produces a reviewable diff rather than a claim about feel.

---

## Known limitations (real, not yet worth fixing)

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
  camera maths, smoothing, player movement, turning and the yaw convention are
  covered; the cube mesh is not. The yaw test is the only pixel-level check that
  the image is *correct* rather than merely accepted — `gfx/capture.rs` is the
  machinery, and the cube's winding and 24-vertex normals are the obvious next
  customers.
- Perf numbers must be taken with the window visible. An occluded surface hands
  back no texture, the draw is skipped, and the loop then spins as fast as it
  likes — which reads as a spectacular frame rate for drawing nothing. `state`
  reports `skipped` so that case is self-diagnosing rather than mysterious.
- The headless test needs a real adapter, so `cargo test` will not pass in an
  environment without a GPU.
