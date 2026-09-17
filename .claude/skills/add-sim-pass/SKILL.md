---
name: add-sim-pass
description: Add or change simulation passes, including movement, contact, attacks and spawning. Use when changing World::step or the behaviour it schedules; gameplay relationships belong in game.
---

# Add or change a simulation pass

Read [repository rules](../../../CLAUDE.md), the owning pass and its neighbours
in [the schedule](../../../crates/sim/src/pass/mod.rs). Explain what rule changes,
why it runs at that point, and an observable consequence for play.

## Access and composition

A pass takes only the slices and capabilities it needs. Use the current pass
signatures as examples; do not pass an unrestricted mutable World or Game.
For example, attack reads positions and submits momentum through `ImpulseSink`;
physics integrates it later. This allows other producers to use the same motion
rules and lets contacts propagate an attack's consequences.

Keep mechanic logic in its module. Add only wiring to `World::step`, and update
the schedule explanation with the reason for the ordering. State which tick's
facts the pass reads and when its effects become visible. The schedule prose
is not mechanically synchronized with the call order; verify both.

Use `Dt` when a pass needs time, never an arbitrary frame duration. Keep tuning
beside its owner, with range validation at the shared entry point. Use const
assertions for static bounds and validated types for runtime values.

Structural edits must respect row lifetimes: spawn draining precedes row readers,
and defeated-body removal follows them. Retain stable entity IDs across removals,
not dense indices. New membership or state must participate in cleanup, scene
lifetime, restart, reporting and hashing.

## Observation

Expose read-only state for assertions. Emit typed transitions through `TraceSink`
when timing or history matters; the sink supplies the tick. Gameplay must never
read the diagnostic trace to decide what happens next.

Aggregate repetitive contacts per tick; preserve individual events when identity
matters, such as damage or removal. Trace externally driven state changes too.
Exhaustive report/hash destructuring prompts an update when storage changes,
but tests must verify that the new data actually reaches those outputs.

## Verification and teaching

Use the [scenario skill](../scenario/SKILL.md). Predict the isolated behaviour
and a relevant interaction with another system. Assert tick boundaries and the
negative case where a result must not occur. For a horde-scaling pass, include a
step-time budget and validate performance in release.

Run all [repository checks](../../../CLAUDE.md#checks-and-commands). Review golden
trace changes against the intended rule before blessing them. Explain the
result in terms of cause and effect, what the checks prove, and any remaining
feel judgement for the user to try in a live playtest.
