---
name: scenario
description: Write, run and update arpg scenarios — the headless, exit-code-gated tests that drive the sim with a scripted input stream and assert over final state and the trace. Use whenever verifying a change to simulation behaviour, adding a regression test for a bug, updating golden traces, or asking whether a change is done. Prefer this over driving the game through the ARPG_HARNESS socket for anything that can be checked without a GPU or a window.
---

# Scenarios

A scenario is the unit of verification in this repo. Setup, an input stream
measured in ticks, assertions over final state and over the trace, a tick
budget. It runs headless against `sim` — no GPU, no window, microseconds per
run — and it exits 0 or 1.

The harness socket (`playtest` skill) stays useful for the interactive and
visual cases, where a real window is the point. It is not the completion gate —
rule 4 in `CLAUDE.md` says why.

```sh
. "$HOME/.cargo/env" && cargo run --quiet -p scenario -- scenarios/
echo $?          # 0 or 1 — this is the gate
```

The `Stop` hook runs this when a turn ends and blocks on failure.

## Shape

```ron
// scenarios/walk_east.ron
(
    description: "One sentence on what this would catch.",
    setup: (enemies: 0),
    // Inputs are indexed in ticks, never milliseconds. Wall clock has no
    // meaning here: the whole run is a loop over World::step. Spans apply in
    // order, so a later one overrides an earlier one where they overlap.
    inputs: [
        (at: 0, ticks: 30, dir: (1.0, 0.0)),
    ],
    budget: (ticks: 30),
    expect: (
        player_pos: (x: 4.5, z: 0.0, tol: 0.001),
        facing:     (value: 1.5708, tol: 0.0001),
        contacts:   0,
    ),
)
```

**`dir` is world space, not screen space.** The game maps screen-right onto the
world diagonal `(+X, -Z)/√2`, but that mapping belongs to the camera — it
depends on the camera's *angle*, which is a presentation decision that may still
change. A scenario asserts what the simulation does, so it speaks the
simulation's axes and survives the view rotating. `(1, 1)` is normalised on the
way in, so a diagonal moves at full speed rather than √2 times it.

**`enemies: 0` is the useful default.** The horde spawns centred on the origin
and so does the player, so *any* horde puts the two in contact on tick zero —
and a prediction about movement then becomes a prediction about the contact
solver, which cannot be made by hand. Spawn a horde when the horde is the
subject.

## What can be asserted today

`player_pos` (as `(x, z)` with a radius tolerance), `facing` (radians,
`(value, tol)`), `contacts`, `enemy_count`. Every one is optional. The tick
budget is always checked — the run must take exactly that many ticks.

Plus `trace: "name.trace"`, a checked-in golden file the run's trace must match
exactly — see below.

Not yet: pointwise trace assertions (`(tick: 417, event: "hitbox.active")`;
golden files cover the same ground for now), anything about the image (chunk 6),
and placing the player or spawning a body anywhere but the default grid
(chunk 4). If a scenario needs one of those, say so rather than working
around it with a warm-up that makes the prediction unreadable.

## Every scenario is also a replay test

The runner runs each scenario **twice** and compares `World::hash()` tick by
tick, whether or not the scenario asks. A divergence reports the tick it
happened on:

```
FAIL  the_horde_stays_deterministic
        replay
          expected: two runs identical every tick
          actual:   diverged at tick 1
```

This is free — a whole run is microseconds — and it means determinism is checked
by every scenario anyone writes for any other reason, which is the only way a
property that subtle stays checked at all.

## Predict first, then assert

Work the expected value out from the constants before running anything. A match
is evidence; a mismatch tells you which of the two is wrong. Running first and
pasting in whatever came out produces a file that asserts the current behaviour
is the current behaviour, which will pass forever and catch nothing.

Re-read the constants rather than trusting any list of them — they are tuning
knobs and they move. `PLAYER_SPEED`, `PLAYER_TURN_RATE` and `ARENA_HALF` are in
`crates/sim/src/lib.rs`; the camera half-lives are in `crates/gfx/src/camera.rs`
and are presentation, so they must not appear in a scenario assertion.

Yaw is `atan2(dir.x, dir.z)`, so walking `(1, 0)` ends up facing `PI/2 ≈
1.5708`. Walking is `PLAYER_SPEED * Dt::SECS` = **0.15 units per tick**, which
is the number to do arithmetic in.

Two worked examples, because the arithmetic is the part people skip:

- 30 ticks at `(1, 0)` is `30 * 0.15 = 4.5` along +X. Facing has to cover
  `PI/2 = 1.5708` at `PLAYER_TURN_RATE` 14 rad/s, which is `14/60 = 0.2333` per
  tick, so it arrives on tick 7 and clamps.
- Walking into the wall stops at `ARENA_HALF - PLAYER_RADIUS = 96.0 - 0.3 =
  **95.7**`, not 96.0. It is the *body* that stops. A scenario asserting 96.0
  would be asserting that the player stands halfway inside the wall.

Under a fixed timestep the result is exact, so tolerances exist for float
accumulation, not for timing slop. A tolerance wide enough to hide a one-tick
error is not a tolerance — and one tick is 0.15, so a tolerance anywhere near
that is the assertion switched off. `tol: 0.0` is legitimate and used.

## Asserting over the trace

Today this is done with a **golden file** (below). Pointwise assertions —
`(tick: 417, event: "hitbox.active")` — are not implemented; writing one into a
scenario is refused by `deny_unknown_fields` rather than silently ignored, which
is deliberate.

Final state cannot see timing. Anything with a window — attack startup, an
active hitbox, hitstop, a buffered input — is asserted against the event stream:

```ron
trace: [
    (tick: 417, event: "hitbox.active"),
    (tick: 418, event: "hit", entity: 93),
    (tick: 429, event: "hitbox.inactive"),
]
```

Assert the *ticks*, not merely that the events occurred in order. A hitbox live
for thirteen frames instead of twelve produces the same ordered sequence and a
different game.

Also assert the negative where it is the point: a hit landing one tick outside
the window must **not** register.

## Golden traces

For behaviour too broad to enumerate, check in the whole trace and diff against
it. A tuning change then produces a reviewable diff instead of a claim.

Regenerate deliberately, never reflexively:

```sh
cargo run --quiet -p scenario -- scenarios/knockback.ron --bless
```

A golden trace records the **run**, not the setup: the runner clears the trace
after building the world, so a golden file is not coupled to `DEFAULT_ENEMIES`
or anything else about construction.

If the ring buffer wrapped, the runner refuses to compare rather than blessing a
truncated file. Shorten the scenario instead.

**Read the diff before committing it.** A blessed golden file that nobody looked
at is a test that has been deleted without anyone noticing. If the diff is
larger than the change should produce, that is the finding.

## Determinism failures

A determinism failure does not show up as a scenario that passes
*intermittently* — the replay check runs both passes back to back, so it fails
the same way every time. When it does, the sim has picked up a source of
nondeterminism and that is the bug, not the scenario. The runner's own note
lists the suspects: a wall clock, a bare `f32` where a `Dt` belongs, iteration
over a hash-ordered container, unseeded randomness, or presentation state
feeding back into sim state.

The reported tick is the useful part. "Diverged at tick 1" and "diverged at tick
340" are different bugs.

## Adding a scenario for a bug

Before fixing it, write the scenario that reproduces it and watch it fail.
Afterwards, append an entry to `docs/traps.md` if the symptom was misleading
rather than obvious — and note there that the scenario is now the enforcement,
which is a promotion off layer 4.

## What scenarios cannot tell you

- **Feel.** They prove the impulse was 4.1 at tick 418. Whether 4.1 is right is
  the owner's call, at the keyboard, under vsync.
- **Rendering.** Anything about the image needs the GPU path — the yaw pixel
  test is the existing pattern, and offscreen capture (roadmap chunk 6) is what
  will make image checks available to a scenario at all.
- **Input latency and OS event delivery.** Scenarios inject at the action layer.
