---
name: scenario
description: Write, run and update headless scenarios for simulation and gameplay behaviour, regressions, timing assertions and golden traces. Use live playtesting for rendering and device integration.
---

# Scenarios

Scenarios drive `Game::step` without a GPU, window or wall-clock pacing. They
assert checkpoints, final state and optional golden traces. Every run also
replays and compares Game hashes tick by tick. Repeatability alone does not
prove the intended behaviour, so explicit predictions are required.

```sh
. "$HOME/.cargo/env"
cargo run --quiet -p scenario -- scenarios/
```

Run the full scenario gate after the focused case, then the other
[repository checks](../../../CLAUDE.md#checks-and-commands). Hooks are specific to
Claude Code; other agents must run the commands explicitly.

## Start from the owning definitions

Read [Spec and Expect](../../../crates/scenario/src/spec.rs) for supported
commands and assertions, and use a relevant checked-in scenario as a starting
point. Sim/game types own the values those commands consume. Unknown fields
are rejected; do not invent syntax or duplicate an owning schema in the runner.

A minimal movement example:

```ron
(
    description: "Walking east covers the expected distance.",
    setup: (enemies: 0),
    inputs: [(at: 0, ticks: 30, dir: (1.0, 0.0))],
    budget: (ticks: 30),
    expect: (
        player_pos: (x: 4.5, z: 0.0, tol: 0.001),
        facing: (value: 1.5708, tol: 0.0001),
        contacts: 0,
    ),
)
```

`dir` is world space, with ground coordinates `(x, z)`. Screen-direction mapping
belongs to app/camera. Start with zero enemies unless the crowd is the subject;
otherwise contact can change a movement prediction. Place bodies deliberately
through setup actions or a scene rather than depending on the default grid.

Read tuning at its owner before calculating an expectation: movement in
[walk](../../../crates/sim/src/pass/walk.rs), turning in
[face](../../../crates/sim/src/pass/face.rs), bounds in
[World](../../../crates/sim/src/lib.rs). In the example, distance is speed times
fixed tick duration times thirty. Explain that calculation to the user when it
helps them understand the behaviour. Keep float tolerances smaller than the
one-tick error the assertion should detect.

## Time and identity

Inputs and commands use zero-based ticks. A checkpoint at `at: 0` observes the
first completed step, not untouched setup. Checkpoints must be below the tick
budget. A failure remains a failure even if final state recovers or `--bless`
is supplied.

Use intermediate checkpoints on both sides of a transition. See
[attack timing](../../../scenarios/the_hitbox_opens_and_shuts_on_schedule.ron).
Golden traces belong in final `expect.trace`, never a checkpoint. Pointwise
trace-assertion syntax is not supported.

Body assertions refer to placement order via `nth`; the runner resolves stable
identities so removals do not redirect an assertion to a swapped row. Scene
indices refer to load order. Read [scene semantics](../../../docs/scene-playtests.md)
when combining setup and timeline loads. Missing targets and unreachable
commands must fail rather than silently disappear.

## Composition examples

- [Physical motion](../../../scenarios/an_impulse_moves_and_damps.ron):
  `impulses: [(at: 0, target: Placed(0), value: (6.0, 0.0))]` submits momentum;
  `Player` targets the player. Assert carried velocity and later displacement.
  Powered walking and seeking are separate from carried velocity.
- [Source enablement](../../../docs/source-enablement.md): switches run before
  source evaluation; assertions consume the engine-owned `SourceState`.
- [Source control](../../../docs/source-control.md): complete inline scenes use
  `Gameplay(...)`; legacy physical scenes use `Inline(...)`. Assert activation
  on N and permitted emission on N+1, as well as one-shot consumption and lifetime.
- [Scene authoring](../create-scene/SKILL.md): load the playable file through
  `File(...)` so the tested content is the content the user plays.

For a new rule, assert its own result and an interaction with an existing rule.
For a regression, demonstrate the failure before fixing it where practical.
Rejection, stale IDs, eviction and restart need checks when the change touches
those boundaries. Restart is tested through public Game lifecycle tests; the
scenario timeline does not offer a restart command.

## Golden traces and performance

Set `expect.trace` to a trace filename relative to the scenario. The runner
clears setup events before recording the run and refuses truncated trace
comparison. To regenerate a deliberately changed expectation:

```sh
cargo run --quiet -p scenario -- scenarios/the_hitbox_opens_and_shuts_on_schedule.ron --bless
```

Read the diff against the predicted event ticks, identities and negative cases.
Do not bless an unexplained difference. A final state can look correct despite
an extra hit or an early spawn; that is why the event sequence matters.

`budget.max_mean_step_micros` measures Game stepping, excluding setup, hashing
and assertions. Use release for performance conclusions. It measures mean step
cost, not worst-tick latency or render performance.

Replay divergence suggests nondeterministic input, ordering, randomness or
presentation feedback. Use the first divergent tick to investigate; repeated
runs can miss nondeterminism, so passing replay is evidence rather than a proof
for every possible execution.

Scenarios cannot judge combat feel, rendering, OS input delivery or real input
latency. Use the [playtest skill](../playtest/SKILL.md) for live checks and report
those results separately from durable behaviour assertions.
