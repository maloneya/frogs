---
name: create-scene
description: Build or revise playable arpg scenes through the shared headless API, native editor or RON files; playtest compositions and preserve intended gameplay with scenarios.
---

# Create a scene

Scenes in `scenes/` are game content. Scenarios in `scenarios/` are automated
inputs/assertions. Use the [scene editor guide](../../../docs/scene-editor.md)
for the human build–play–adjust loop and current tooling limits.

## Choose the composition

State what the player should try and what the scene should reveal. Put a short
purpose/playtest note in leading line comments, which editor saves preserve.
Use deliberate placements; keep unrelated content constant when comparing variants.
Proceed with reasonable defaults unless a missing choice changes the trial materially.

Prefer composing existing rules over adding engine code for an encounter. Use
**Save as…** to explore a scene already referenced by regression tests. Identify
missing authoring controls separately from genuinely missing gameplay rules.

## Author with one vocabulary

Use the [headless authoring API](../../../docs/scene-authoring.md) for the same
edits and admission as the companion, without a running game. Direct RON editing
uses the same document format; validate it with the CLI's `open` action.

Files wrap playable data under `scene` with optional authoring `labels`.
Read [GameScene](../../../crates/game/src/scene.rs) and the relevant owners:
[physical Scene/BodyGrid](../../../crates/sim/src/scene.rs),
[Template](../../../crates/sim/src/pass/spawn.rs),
[SourceSpec](../../../crates/sim/src/pass/source.rs), and
[encounters](../../../docs/encounters.md) for relationships. Read tuning from its
owner when predicting an outcome. Existing files are examples, not a schema authority.

Use `grids` for regular crowds, individual `bodies` for deliberate placements.
Name the scene and save a `.ron` regular file directly inside `scenes/` for catalog
discovery. Name important objects so encounter choices are recognizable. Bindings
use authored indices; shared edits remap them, while manual reordering requires care.
Inline comments are lost on editor save, so do not rely on them to explain manual
index bindings. Runtime tuning is saved separately from the scene.

## Verify and hand off

Validate/load the authored content and use the [playtest guide](../playtest/SKILL.md)
when appearance or feel is the question. Exploratory placement and lighting do
not need a new automated scenario for every iteration. When a gameplay composition
is intended to remain reliable, create/update assertions loading the actual file:

```ron
setup: (scenes: [File("../scenes/name.ron")]),
```

Do not copy its layout into a second setup. The [scenario guide](../scenario/SKILL.md)
covers predictions, intermediate ticks, refusal and replay; a checkpoint at zero
observes the first completed step. Changes to simulation/gameplay rules still
require passing scenarios. Review any existing tests that reference edited files;
do not bless failures without understanding the changed outcome.

Run the applicable [repository checks](../../../docs/verification.md). Hand off
what to try, what was verified and the remaining feel question. Link to the
[editor workflow](../../../docs/scene-editor.md#build-play-adjust) for opening,
saving, playing and repeating it rather than duplicating those instructions.
