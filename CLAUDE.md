# Working on arpg

A handwritten action-RPG engine for learning, focused on combat against crowds.
Target: native macOS / Apple M4 / Metal. Cross-platform and wasm are out of scope.
Start with [README](README.md) for build, play and controls.

## Design priorities

- Compose and play scenes before extending the engine. Add capabilities for a
  concrete playtest need; update persistence and both authoring clients together.
- Sim owns shared physics; game owns mechanic relationships. Keep mechanisms
  below their producers so attacks, movement and contacts compose.
- Hand-write the loop, entity storage, spatial partitioning and renderer.
  Math/plumbing crates are fine; an off-the-shelf engine or ECS is not.
- Explain the observable effect and tradeoff in plain language. Distinguish
  verified behaviour from human judgement of feel.

## Rules while editing

- Enforce invariants at the shared owner, including file/harness admission.
  Prefer private state and validated types over caller instructions. Keep types,
  validation and tuning in one place; adapters consume them.
- Simulation/gameplay use ordered inputs, never ambient time or randomness.
  Presentation and diagnostics cannot drive gameplay. Replay covers one binary
  on one machine.
- Step methods wire named passes with narrow slices/capabilities, not mutable
  World/Game access. Keep timing rationale beside the relevant calls.
- New state needs spawn/removal, scene eviction, restart, hash and report handling.
  Validate complete loads before mutation; refusal preserves accepted state/work.
- Behaviour changes need predicted outcomes through Game, including relevant
  intermediate ticks, interactions, refusals and stale IDs. Tests and benchmarks
  own their setups; never load playable `scenes/` files as test fixtures.
- On a performance regression or credible performance concern, stop feature
  work and flag it immediately. Preserve changes and evidence; do not optimize,
  retry toward a pass or expand scope. Follow the [stop and handoff policy](docs/performance.md#feature-work-stop-and-handoff).
  Investigation starts only when the user requests it; implementation waits for
  the user's choice of approach.
- Search [traps](docs/traps.md) when diagnosing. Record symptom, cause and check
  after a misleading failure costs ten minutes; replace recurring advice with checks.
- No compatibility shims, aliases, legacy readers or migration fallbacks without
  a concrete user need. Update consumers and delete the obsolete path.
- Preserve [world units](docs/world-units.md), linear colours with sRGB output,
  elapsed-time smoothing, bounded shared crowd poses, continuous redraw and
  discarded depth. Read the relevant owner/tests before changing these contracts.

## Checks and commands

```sh
python3 scripts/dev.py          # game + companion, independently running
python3 scripts/verify.py full --snapshot  # verify captured inputs while edits continue
```

Rust may need `. "$HOME/.cargo/env"`. Use release builds for performance.
Follow [verification policy](docs/verification.md) for check selection and evidence;
report failures, uncertainty and omitted coverage. Never relax a lint, dependency
rule, test or performance gate just to pass. Use justified `#[expect]` exceptions.

Drive live checks through `ARPG_HARNESS`, not OS key injection or desktop capture;
see [playtesting](.claude/skills/playtest/SKILL.md).

## Task guides

Read only what the task needs:

- [Create a scene](.claude/skills/create-scene/SKILL.md), [editor](docs/scene-editor.md), [headless API](docs/scene-authoring.md), [scene lifetime](docs/scene-playtests.md)
- [Write scenarios](.claude/skills/scenario/SKILL.md), [live playtests](.claude/skills/playtest/SKILL.md)
- [Add/change a sim pass](.claude/skills/add-sim-pass/SKILL.md)
- [Investigate performance](.claude/skills/investigate-performance/SKILL.md): user-triggered evidence, published approaches and a decision brief
- [Character workflow](.claude/skills/character-workflow/SKILL.md): references, visual development and review
- [Blender assets](.claude/skills/author-blender-asset/SKILL.md), [asset contract](docs/character-assets-authoring.md)
- [Encounters](docs/encounters.md), [interaction](docs/interaction.md), [source enablement](docs/source-enablement.md)
- [Collision debug](docs/collision-debug.md), [attack effects](docs/attack-effects.md)

Keep only non-obvious rationale beside code. Link to owners instead of copying
schemas, schedules, tuning catalogs or test inventories.
