---
name: scenario
description: Write, run and update headless scenarios for simulation and gameplay behaviour, regressions, timing assertions and golden traces. Use live playtesting for rendering and device integration.
---

# Headless scenarios

Playable scenes are game content; scenarios own their setups and test rules
and a small set of complex test-only compositions. They never load `scenes/` files.
They drive
`Game::step` without a GPU, window or wall-clock pacing. They
assert checkpoints, final state and optional golden traces. Every run also
replays and compares Game hashes tick by tick. Repeatability alone does not
prove the intended behaviour, so explicit predictions are required.

```sh
. "$HOME/.cargo/env"
cargo run --quiet -p scenario -- scenarios/
```

Run a focused case while iterating, then the shared
[full verification](../../../docs/verification.md), which includes all scenarios.

## Start from the owning definitions

Read [Spec and Expect](../../../crates/scenario/src/spec.rs) for supported
commands and assertions, and use a relevant checked-in scenario as a starting
point. Sim/game types own the values those commands consume. Unknown fields
are rejected; do not invent syntax or duplicate an owning schema in the runner.

A minimal movement example:

```ron
(
    description: "Walking east covers the expected distance.",
    inputs: [(at: 0, ticks: 30, dir: (1.0, 0.0))],
    budget: (ticks: 30),
    expect: (
        player_pos: (x: 3.5141285, z: 0.0, tol: 0.001),
        facing: (value: 1.5708, tol: 0.0001),
        contacts: 0,
    ),
)
```

For independent aim use `aims: [(at: 0, dir: (0.0, 1.0))]`; each finite,
nonzero direction is retained until the next aim command. Without aim commands,
the keyboard driver uses nonzero movement and retains its last direction when
stationary, initially +Z. Every tick supplies a required `AimDir` to `Game`.
See [retreating attacks](../../../scenarios/retreating_attacks_keep_their_aim.ron).

`dir` is world space, with ground coordinates `(x, z)`. Screen-direction mapping
belongs to app/camera. Start with zero enemies unless the crowd is the subject;
otherwise contact can change a movement prediction. Place bodies deliberately
through setup actions or a scene; a fresh Game starts empty.

Read [tuning](../../../crates/sim/src/tuning.rs), [turning](../../../crates/sim/src/pass/face.rs)
and [bounds](../../../crates/sim/src/lib.rs) before predicting. Distance above
includes motor acceleration and drag before reaching target speed; see the numeric
derivation in `scenarios/walk_east.ron`. Keep tolerances below the one-tick error
the assertion should detect.

## Time and identity

Inputs and commands use zero-based ticks. A checkpoint at `at: 0` observes the
first completed step, not untouched setup. Checkpoints must be below the tick
budget. A failure remains a failure even if final state recovers or `--bless`
is supplied.

Use intermediate checkpoints on both sides of a transition. See
[attack timing](../../../scenarios/the_hitbox_opens_and_shuts_on_schedule.ron).
Whole-run transition assertions belong in final `expect.events`, never a checkpoint.
Use explicit kinds and an exact sequence, including kinds expected to be absent:

```ron
events: (
    kinds: [Swung, HitboxOpened, HitboxClosed, Hit],
    sequence: [(0, Swung), (6, HitboxOpened), (12, HitboxClosed)],
),
```

This predicts exactly one attack window and no hits; extra, missing, early or
late events fail. Events outside the selected kinds are irrelevant. Payloads
are typed and checked, including diagnostic identities `(slot, generation)`;
these read-only pairs cannot be turned into engine handles.

Body assertions refer to placement order via `nth`; the runner resolves stable
identities so removals do not redirect an assertion to a swapped row. Scene
indices refer to load order. Read [scene semantics](../../../docs/scene-playtests.md)
when combining setup and timeline loads. Missing targets and unreachable
commands must fail rather than silently disappear.

Every command tick must be below the budget. Movement and channel spans must be
nonempty and fit completely inside it; the runner never truncates a span. A
zero-tick budget is valid for setup-only assertions. Numeric predictions must
be finite and tolerances finite and nonnegative, including event payloads and
checkpoints. Requesting a dead body's position fails even alongside `alive: false`.

## Composition examples

- [Physical motion](../../../scenarios/an_impulse_moves_and_damps.ron):
  `impulses: [(at: 0, target: Placed(0), value: (6.0, 0.0))]` submits momentum;
  `Player` targets the player. Assert physical velocity and later displacement.
  Player control movement is separate; autonomous movement uses a bounded motor on the same
  velocity as impulses. See [crowd motion](../../../docs/crowd-collision.md#movement-and-pushing).
- [Source enablement](../../../docs/source-enablement.md): switches run before
  source evaluation; assertions consume the engine-owned `SourceState`.
- [Encounters](../../../docs/encounters.md): `setup.scenes` and timeline `Load(...)`
  contain test-owned `GameScene` values directly, with `engine` at the top level.
  Assert activation on N and permitted emission on N+1, as well as one-shot
  start, deadline, reward and lifetime. External scene references are not supported.
- [Complex integration](../../../scenarios/two_encounters_resolve_independently.ron):
  combine rules only when the interaction is the subject. Keep ordinary system
  cases small; do not reproduce a playable level to test one mechanism.

For a new rule, assert its own result and an interaction with an existing rule.
For a regression, demonstrate the failure before fixing it where practical.
Rejection, stale IDs, eviction and restart need checks when the change touches
those boundaries. Restart is tested through public Game lifecycle tests; the
scenario timeline does not offer a restart command.

## Transition evidence and performance

Prefer `expect.events` for mechanics. The collector selects events as they are
emitted and keeps only the expected count plus one, failing explicitly on any
additional overflow. Clearing, disabling, or wrapping recent diagnostic history
cannot affect assertions. A selected kind with no sequence entries asserts absence.

The remaining `expect.trace` goldens check diagnostic serialization and gameplay
streams. They use independent bounded capture of every stream and reject overflow;
they do not read the live history ring. Do not combine `events` and `trace`.
Set `expect.trace` to a filename relative to the scenario. Setup events are excluded.
To regenerate a deliberately changed serialization expectation:

```sh
cargo run --quiet -p scenario -- scenarios/the_hitbox_opens_and_shuts_on_schedule.ron --bless
```

Read the diff against the predicted event ticks, identities and negative cases.
Do not bless an unexplained difference. A final state can look correct despite
an extra hit or an early spawn; that is why the event sequence matters.

`budget.max_mean_step_micros` measures Game stepping, excluding setup, hashing
and assertions. Use release for performance conclusions. It measures mean step
cost, not worst-tick latency or render performance.

Investigate replay divergence at the first differing tick: input, ordering,
randomness or presentation feedback. For rendering/input integration use the
[playtest guide](../playtest/SKILL.md); combat feel needs a human trial.
