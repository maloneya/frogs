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

## 1. Fixed timestep and render interpolation — *sim layer*

`Dt` and `Alpha` as newtypes with private constructors, so a variable timestep
cannot be smuggled in and interpolation cannot leak into sim state. The
accumulator lands in `sim`, next to the `Dt` it mints. `World::hash()` over all
sim state.

**Gate:** a test replaying one recorded input stream twice asserts an identical
per-tick hash sequence, not merely identical final positions. An allocation
counter asserts a steady-state frame allocates zero.

## 2. Trace stream — *perception*

Tick-stamped typed events in a ring buffer; `trace since <tick>` on the harness
socket; written to file by scenarios once those exist.

Lands *before* the first hitbox rather than after. Attack windows, hitstop and
knockback fail without a crash or a compiler error, and a state snapshot taken
afterward cannot see a mistimed frame inside an active window.

**Gate:** movement and contact events appear in a checked-in golden trace, and a
deliberate off-by-one in a pass changes that file.

## 3. Scenario runner — *scenarios*

`crates/scenario`, depending on `core` + `sim` only. RON setup, tick-indexed
inputs, assertions over final state and trace, a tick budget. GPU-dependent
tests move behind `#[cfg(feature = "gpu")]`.

**Gate:** `cargo run -p scenario -- scenarios/` exits 0 with no GPU and no
window, and `cargo test --workspace` passes on a machine without an adapter. The
`Stop` hook stops being conditional.

## 4. SoA entity storage and pass decomposition — *hooks*

`EntityId` from `World::spawn`, plus `spawn`/`place` as scenario setup
primitives. Same chunk as the pass split, because SoA is what makes disjoint
slices exist: `step` becomes an ordered list of named passes, each taking the
slices it declares, one module each.

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
