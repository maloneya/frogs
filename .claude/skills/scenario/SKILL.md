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
visual cases, where a real window is the point. It is not the completion gate.
Reading `player_pos 3.183` and judging it correct is the agent grading its own
work; the same prediction written as an assertion is durable and checkable, and
it costs the same to write.

```sh
. "$HOME/.cargo/env" && cargo run --quiet -p scenario -- scenarios/
echo $?          # 0 or 1 — this is the gate
```

The `Stop` hook runs this when a turn ends and blocks on failure.

## Shape

```ron
// scenarios/walk_east.ron
(
    setup: (
        seed: 0,
        enemies: 64,
    ),
    // Inputs are indexed in ticks, never milliseconds. Wall clock has no
    // meaning here: the whole run is a loop over World::step.
    inputs: [
        (at: 0, hold: (D, 30)),
    ],
    budget: (ticks: 60),
    expect: (
        player_pos: ((3.182, -3.182), tol: 0.01),
        facing:     (2.3562, tol: 0.001),
        contacts:   0,
    ),
)
```

## Predict first, then assert

Work the expected value out from the constants before running anything. A match
is evidence; a mismatch tells you which of the two is wrong. Running first and
pasting in whatever came out produces a file that asserts the current behaviour
is the current behaviour, which will pass forever and catch nothing.

Re-read the constants rather than trusting any list of them — they are tuning
knobs and they move. `PLAYER_SPEED`, `PLAYER_TURN_RATE` and `ARENA_HALF` are in
`crates/sim/src/lib.rs`; the camera half-lives are in `crates/gfx/src/camera.rs`
and are presentation, so they must not appear in a scenario assertion.

Screen directions are world diagonals: screen-right is `(+X, -Z)/√2`, screen-up
is `(-X, -Z)/√2`. Yaw is `atan2(dir.x, dir.z)`, so due-east is `3π/4 ≈ 2.3562`.

Under a fixed timestep the result is exact, so tolerances exist for float
accumulation, not for timing slop. A tolerance wide enough to hide a one-tick
error is not a tolerance.

## Asserting over the trace

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
cargo run -p scenario -- scenarios/knockback.ron --bless
```

**Read the diff before committing it.** A blessed golden file that nobody looked
at is a test that has been deleted without anyone noticing. If the diff is
larger than the change should produce, that is the finding.

## Determinism failures

If a scenario passes intermittently, the sim has picked up a source of
nondeterminism and that is the bug — not the scenario. Look for: a wall clock, a
bare `f32` where a `Dt` belongs, iteration over a hash-ordered container,
unseeded randomness, or presentation state feeding back into sim state.

The replay scenario is the sharpest instrument here: it asserts the per-tick
hash sequence, so it reports the tick at which two runs diverged rather than
just that they did.

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
