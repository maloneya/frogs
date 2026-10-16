# Authoring encounters

Within a [scene document](scene-authoring.md#document-and-runtime-boundary),
the `scene.encounters` field contains the gameplay composition. Physical placements remain
under `engine`; `doors` bind movable props to physical panels, while `chests` bind
fixed props to gameplay components. The example below binds eight sources to
a defeat objective and rewards a chest opening. Multiple
entries run independently in the same scene, and loading the scene twice makes
independent copies of every relationship.

Build complete relationships in the [editor](scene-editor.md#compose-an-encounter)
or [headless API](scene-authoring.md). The RON below belongs inside `scene`.
[EncounterSpec](../crates/game/src/encounter.rs), [DoorSpec](../crates/game/src/door.rs)
and [ChestSpec](../crates/game/src/chest.rs) own the schema.

```ron
encounters: [(
    name: "Soul trial",
    start: Activated(0),
    objective: Defeat((sources: [0,1,2,3,4,5,6,7], count: 30, within_ticks: 1800)),
    on_success: [UnlockChest(0)],
)],
```

The numbers bind to **authored scene entries**, never current dense storage rows:
`Activated` addresses an explicit engine body; the objective
addresses engine sources; `UnlockDoor` addresses `doors`; `UnlockChest` addresses `chests`.
References are resolved once to generational body IDs and monotonic source IDs.
Manual reordering can change valid bindings; review every referenced index.
Keep binding explanations in leading comments, since editor saves drop inline
comments. Editor deletion remaps surviving links or refuses required endpoints;
duplication copies door/chest components but leaves encounter links on the original.
Object names label editor choices; encounter names are unique display labels.

## What composes

- `Activated` waits for the existing one-shot interaction mechanism.
- `Defeat` counts combat defeats from a group of one or more sources.
- Starting an encounter enables its objective source group once, in authored
  group order, using normal engine cadence and placement rules. Sources must
  initially be disabled and produce enemies. Optional `on_start` effects can
  unlock doors or chests.
- `UnlockDoor` releases the physical latch of a [hinged panel](physical-doors.md).
  It adds no motion and keeps the body present. Contacts and attack impulses
  move the released panel; sufficient physical load can also break its latch
  before the encounter releases it.

- `UnlockChest` grants interaction to a locked chest. A fresh nearby E press
  opens its lid on the next gameplay pass. The body stays present, with the
  same circular collision footprint; opening has no loot or inventory effect.
  Declare `chests: [(body: 1)]` alongside `engine`. Admission requires an
  explicit non-interactable fixed body, rejects duplicate bindings and bodies
  shared with doors, and enforces game-owned `MAX_CHESTS` across live scenes.

A source group is a set of producer identities, not a population counter. The
engine records provenance when a spawn actually lands, then carries it into
lossless defeat outcomes. Manual removal, scene eviction, unrelated enemies,
and recycled body slots cannot award progress. `FewerThan` still gates on the
world's enemy population; the playable trial uses cadence alone so another
encounter cannot hold its source closed through that global condition.

Every source and controlled door or chest has one gameplay controller within the scene.
Admission rejects conflicting encounters,
invalid endpoints, duplicate effects, zero targets/durations and excess live
encounters before installation mutates anything. Only the encounter's objective group is enabled at start. Terminal effects
unlock doors or chests; they cannot re-enable sources.

All terminal outcomes automatically stop the encounter's sources. Existing enemies remain. Success runs
`on_success`; timeout runs `on_failure`. A missing required endpoint produces
`Orphaned`, stops surviving sources, and runs no reward/failure effects.
After activation is consumed, the switch is no longer a required endpoint.
A released or breached door remains a valid endpoint while its panel exists;
breaching does not orphan the encounter or stop its sources. Terminal records
preserve their historical result. Restart restores the entire cached scene,
including closed, latched doors, locked chests and ready encounters.

## Tick contract

1. Before engine simulation, chests consume the previous tick's interaction.
   Encounters validate endpoints and consume activation, running start effects;
   door release changes the latch directly.
2. Engine simulation emits enemies, moves bodies, resolves combat and interaction,
   then records defeats before removing bodies.
3. After engine simulation, each running encounter consumes that tick's defeat
   outcomes once, spends one active tick, and resolves success before timeout.

If activation occurs on N, a duration D runs on N+1 through N+D inclusive.
A defeat on N+D can win. Thirty seconds is 1,800 simulation ticks at the current
60 Hz rate. Countdown advances with simulation, not wall time. Clearing or
wrapping diagnostic traces cannot affect progression. E activates authored
interaction endpoints; a door swings in response to physical load.

## Observe and verify

`Game::encounter_state(scene, index)` returns `EncounterState`; `Game::door_state` returns `DoorState`; `Game::chest_state` returns
`ChestState`. Doors expose Latched, Released, Broken and Missing; their latch
state does not imply passage clearance. `Game::joint` observes angle, rate and
lock reaction. Chests expose Locked, Unlocked, Open and Missing. Even an open
chest becomes Missing if its body disappears; a reused body slot cannot inherit
its state. Reports expose `encounters`, `doors` and `chests`, keyed by scene/index.
The HUD is a read-only projection of encounter state. `Game::encounter_trace`
records progress, phase, encounter-driven door release and chest transitions;
it is diagnostic only. Physical lock state and load are reported under
`state.sim.joints`. Engine
source operations at the post-step boundary may have the following tick stamp;
separate trace streams do not imply a combined chronology.

Scenarios use the same owning types:

```ron
encounters: [(scene: 0, encounter: 0, state: Some((
    phase: Running, defeated: 0, target: 30, remaining_ticks: 1799,
)))],
doors: [(scene: 0, door: 0, state: Some(Latched))],
```

`None` asserts eviction of an existing authored entry. Invalid indices are
rejected, even when asserting absence or blessing traces. See
[the scenario guide](../.claude/skills/scenario/SKILL.md).
