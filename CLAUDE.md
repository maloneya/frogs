# Working on arpg

A from-scratch action-RPG engine and learning project for a novice developer,
focused on combat against crowds. Understanding the systems matters alongside
getting them running. [README](README.md) is the human entry point.

## Design priorities

- **Build through scenes.** Compose playable content in the [scene editor](docs/scene-editor.md),
  play it, adjust layout/tuning, and repeat. Prefer existing rules and authored
  data; extend code when a concrete playtest needs a missing capability. Scenes
  are freely editable game content; tests own their setups and test the rules. When extending
  authorable content, update persistence and editor support or state the remaining
  authoring gap.
- **Simulation first.** Build consistent rules that compose. An attack submits
  an impulse; shared physics carries it through contacts. Put mechanisms below
  their producers and test combinations players can discover.
- **Teach through the work.** Explain the concept, observable effect and tradeoff
  in plain language. Separate verified behaviour from human feel judgement.
- **Small running changes.** Build one system at a time; no speculative mechanics
  or abstractions. Hand-write the engine layers being studied: loop, entity
  storage, spatial partitioning and renderer. Math/plumbing crates are fine;
  an off-the-shelf engine or ECS is not.

[Design rationale](docs/agent-principles.md) connects these priorities.

## Rules while editing

1. **Enforce at the owner.** Prefer private fields, narrow capabilities and
   validated constructors, then compile-time checks, runtime validation and
   tests. Prose is the last defence. Protect the shared entry point, including
   file and harness input; explain weaker safeguards. See [enforcement](docs/invariants.md).
2. **Keep one definition.** Owners define types, validation and tuning; content,
   UI, reports and harness consume them. Derive facts from authoritative state.
   The companion and agents use the same headless authoring API and complete documents;
   neither owns a second gameplay schema. Every authorable concept must work in both.
3. **Keep execution deterministic.** Simulation/gameplay use state and ordered
   inputs, never wall clock, unseeded randomness or hash-order iteration.
   Presentation cannot write back. Replay is scoped to one binary on one machine.
4. **Declare access and order.** Named passes receive only needed slices or
   capabilities, never unrestricted mutable World/Game. Step methods wire passes,
   not mechanics. State read timing, effect timing and structural-edit ownership.
5. **Make systems independently testable.** Provide typed driving operations,
   read-only observations and transition traces. Gameplay never reads diagnostics.
6. **Close the lifecycle.** Cover spawn, removal, scene eviction, restart, hash
   and report handling. Keep stable identities across dense-row moves. Validate
   complete loads before mutation; refusal preserves live state and accepted work.
7. **Verify outcomes.** Simulation/gameplay behaviour changes need a passing
   scenario through Game, with predictions made before observing results. Check
   intermediate ticks, interactions, refused inputs and stale IDs where relevant.
   Never weaken a gate or bless a trace just to pass. Tests must not load playable
   `scenes/` files. Test systems independently, with a small set of complex
   test-only compositions for interactions; benchmarks own their workloads too.
8. **Diagnose from evidence.** Search [traps](docs/traps.md) first. Record symptom,
   cause and check after a misleading failure costs ten minutes; replace recurring
   advice with executable safeguards.
9. **Replace obsolete paths.** There are no deployed consumers requiring backward
   compatibility. Update callers, data, tools, tests and docs together, then delete
   the old path. No shims, aliases, legacy readers, version branches or migration
   fallbacks unless the user explicitly needs them for a concrete consumer.

## Ownership

Sim owns shared physical mechanisms; game owns mechanic relationships. Both are
deterministic. This engine serves one game: hypothetical reuse is no reason to
abstract. Existing combat tuning still lives beside sim validation; read its
owner before moving it. New gameplay relationships belong in game.

| Crate / tool | Responsibility |
|---|---|
| core | Shared vocabulary, independent of graphics/gameplay relationships |
| sim | Physical world, identity, capabilities, validated operations, pass schedule |
| game | Playable state, mechanic relationships and complete scene lifetime |
| content | Decode/save authoring documents; pass game-owned playable data to runtime |
| authoring | Headless edits, validation, inspection and CLI shared by agents/editor |
| assets | Validate bounded GLB into CPU assets, independent of gameplay/GPU resources |
| gfx | Rendering, camera and GPU resources; receives anonymous geometry |
| app | Devices, presentation, tooling/harness boundary; maps input to world intent |
| scenario | Headless driving and assertions through the same Game used by app |
| tools/companion | Native drafts/undo via authoring; play, tuning and diagnostics via app |

Game privately owns World; app/scenario use Game for construction, stepping and
lifecycle. Engine tests may drive World directly. Crate build scripts enforce
dependency allowlists. The standalone launcher owns process startup; game code
must not launch or locate the companion. See [connection ownership](docs/companion.md).

Native macOS / Apple M4 / Metal is the target; cross-platform/wasm are out of
scope. Preserve [metres](docs/world-units.md), linear colours with sRGB output,
elapsed-time half-life smoothing, bounded shared crowd poses, continuous redraw
and discarded depth after the pass. Read the relevant owner/tests before changing them.

## Checks and commands

```sh
python3 scripts/dev.py          # game + companion, independently running
python3 scripts/verify.py full  # shared verification runner
```

Rust may need `. "$HOME/.cargo/env"`. `cargo run` launches only the game; use
release for performance. [Verification policy](docs/verification.md) owns check
selection, benchmark cadence and evidence. Report failures, inconclusive results
and omitted coverage. Workspace GPU tests require a real adapter; scenarios need
no GPU/window. Use justified `#[expect(lint, reason = "…")]`, never `#[allow]`.
Do not relax lint/dependency policies. Hooks use the same runner but are not proof
that checks ran; inspect saved coverage/source identity. Rustdoc has its own check.

Drive live checks through `ARPG_HARNESS`, not OS key injection or desktop capture;
see [playtesting](.claude/skills/playtest/SKILL.md).

## Task guides

Read only what the task needs:

- [Create a scene](.claude/skills/create-scene/SKILL.md), [editor](docs/scene-editor.md), [headless API](docs/scene-authoring.md), [scene lifetime](docs/scene-playtests.md)
- [Write scenarios](.claude/skills/scenario/SKILL.md), [live playtests](.claude/skills/playtest/SKILL.md)
- [Add/change a sim pass](.claude/skills/add-sim-pass/SKILL.md)
- [Blender assets](.claude/skills/author-blender-asset/SKILL.md), [asset contract](docs/character-assets-authoring.md)
- [Encounters](docs/encounters.md), [interaction](docs/interaction.md), [source enablement](docs/source-enablement.md)
- [Collision debug](docs/collision-debug.md), [attack effects](docs/attack-effects.md)

Keep constraints beside their owner and workflows in task guides. Link instead
of copying schemas, tuning catalogs or test inventories. Remove obsolete prose;
version control holds implementation history.
