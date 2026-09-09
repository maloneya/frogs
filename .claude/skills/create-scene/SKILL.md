---
name: create-scene
description: Create or revise arpg scene files for repeatable fresh playtests, with a scenario that verifies the same authored content. Use for encounter setups and configuration comparisons in scenes/.
---

# Create a scene

Produce a focused playtest configuration whose purpose survives beyond this
conversation. Keep authored content in `scenes/` and assertions in `scenarios/`.

## Decide what the trial should reveal

State the playtest question from the user's request. For example, “three approach
angles to test swing coverage” explains why the configuration exists. Record
that purpose in a comment at the top of the scene, along with the intended player
action and what to watch for. Ask only when a missing choice materially changes
the trial; otherwise state a reasonable assumption and proceed.

Use deliberate placements and only the population needed for that question.
For comparisons, keep unrelated content constant and explain the changed variable
in each file. Separate measurable predictions from feel judgments left to the
person playing.

## Author against the current engine

Read [scene playtests](../../../docs/scene-playtests.md) and the relevant sim
definitions: [Scene and Placed](../../../crates/sim/src/scene.rs),
[Template](../../../crates/sim/src/pass/spawn.rs), and
[SourceSpec](../../../crates/sim/src/pass/source.rs). These types own the
schema; use them over remembered fields or older examples. Read tuning constants
at their definition when calculating a prediction.

Write a descriptively named `.ron` regular file directly inside `scenes/` so the
F2 picker discovers it. Give it a useful display name. Existing files in that
directory are working examples; do not add a duplicate schema or generator.
Keep configuration within the available vocabulary. Identify any missing engine
capability before expanding a scene-authoring task into engine work.

## Verify the same file

Use the [scenario skill](../scenario/SKILL.md) to create or update an assertion
file that loads this scene through `setup.scenes: [File("../scenes/name.ron")]`.
Do not copy its bodies and sources into a second setup. See the
[shared-file example](../../../scenarios/a_playtest_scene_starts_ready_at_tick_zero.ron).

Predict before running. Assert the meaningful setup and any intended sim
behavior, using checkpoints where timing matters. A checkpoint at `at: 0`
observes the first completed step, not untouched setup. A subjective feel trial
still needs checkable conditions that make the trial valid; assertions cannot
prove that combat feels good.

Run the relevant scenario, then the full scenario gate using the commands in
the scenario skill. Resolve failures before calling the configuration ready.

## Hand off a repeatable trial

Use the [playtest skill](../playtest/SKILL.md) when the task needs visual or
interactive verification. Drive the app through its harness.

Explain how to select the scene in F2, what to try, and what was asserted.
Reopen the picker to discover added files. Selecting a file reads its latest
contents; **Restart current** repeats the cached snapshot. After a restart,
discard old run-scoped identities. Report any remaining human feel question
without presenting it as a passed automated check.
