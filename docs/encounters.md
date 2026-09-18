# Authoring encounters

Start with [the playable soul trial](../scenes/soul_trial.ron). Its `encounters`
entry contains the complete gameplay composition. Physical placements remain
under `engine`; `doors` binds static props to the door component. Multiple
entries run independently in the same scene, and loading the scene twice makes
independent copies of every relationship.

For an ordinary new encounter, edit a scene and add a scenario. There is no need
to read the simulation schedule or change the runtime. The authoritative schema
is [EncounterSpec](../crates/game/src/encounter.rs); door content is defined by
[DoorSpec](../crates/game/src/door.rs). The content decoder rejects unknown fields.

```ron
encounters: [(
    name: "Soul trial",
    start: Activated(0),
    objective: Defeat((sources: [0], count: 30, within_ticks: 1800)),
    on_start: [EnableSource(0)],
    on_success: [UnlockDoor(0)],
)],
```

The numbers bind to **authored scene entries**, never current dense storage rows:
`Activated` addresses an explicit engine body; source actions and the objective
address engine sources; `UnlockDoor` addresses the gameplay `doors` list.
References are resolved once to generational body IDs and monotonic source IDs.
Reordering valid authored entries can change their meaning: keep a short label
comment beside endpoints and review bindings when reordering. This first version
uses the existing scene index convention rather than introducing a second name
registry. Names on encounters are display labels, unique within a scene.

## What composes

- `Activated` waits for the existing one-shot interaction mechanism.
- `Defeat` counts combat defeats from a group of one or more sources.
- `EnableSource` uses engine enablement, including its normal
  cadence and placement rules. Sources must initially be disabled and produce
  enemies. Every objective source must be enabled in `on_start`.
- `UnlockDoor` grants interaction to a locked static prop. A later nearby E press
  opens it; the next gameplay pass removes the blocking prop. Opening is a
  greybox removal, with no animated rectangular door asset yet.

A source group is a set of producer identities, not a population counter. The
engine records provenance when a spawn actually lands, then carries it into
lossless defeat outcomes. Manual removal, scene eviction, unrelated enemies,
and recycled body slots cannot award progress. `FewerThan` still gates on the
world's enemy population; the playable trial uses cadence alone so another
encounter cannot hold its source closed through that global condition.

Every source and controlled door has one gameplay controller within the scene.
Admission rejects conflicting encounters, conflicts with legacy source controls,
invalid endpoints, duplicate effects, zero targets/durations and excess live
encounters before installation mutates anything. A source action cannot target
another encounter's source group. Terminal effects cannot re-enable sources.

All terminal outcomes automatically stop the encounter's sources. Existing enemies remain. Success runs
`on_success`; timeout runs `on_failure`. A missing required endpoint produces
`Orphaned`, stops surviving sources, and runs no reward/failure effects.
After activation is consumed, the switch is no longer a required endpoint.
Terminal records preserve their historical result. Restart restores the entire
cached scene, including locked doors and ready encounters.

## Tick contract

1. Before engine simulation, doors consume the previous tick's interaction.
   Encounters validate endpoints and consume activation, running start effects.
2. Engine simulation emits enemies, moves bodies, resolves combat and interaction,
   then records defeats before removing bodies.
3. After engine simulation, each running encounter consumes that tick's defeat
   outcomes once, spends one active tick, and resolves success before timeout.

If activation occurs on N, a duration D runs on N+1 through N+D inclusive.
A defeat on N+D can win. Thirty seconds is 1,800 simulation ticks at the current
60 Hz rate. Countdown advances with simulation, not wall time. Clearing or
wrapping diagnostic traces cannot affect progression. A press while a door is
locked cannot be remembered as an opening request.

## Observe and verify

`Game::encounter_state(scene, index)` returns `EncounterState`; `Game::door_state`
returns `DoorState`. Reports expose `encounters` and `doors`, keyed by scene/index.
The HUD is a read-only projection of encounter state. `Game::encounter_trace`
records progress, phase and door transitions; it is diagnostic only. Engine
source operations at the post-step boundary may have the following tick stamp;
separate trace streams do not imply a combined chronology.

Scenarios use the same owning types:

```ron
encounters: [(scene: 0, encounter: 0, state: Some((
    phase: Running, defeated: 0, target: 30, remaining_ticks: 1799,
)))],
doors: [(scene: 0, door: 0, state: Some(Locked))],
```

`None` asserts eviction of an existing authored entry. Invalid indices are
rejected, even when asserting absence or blessing traces. See
[the scenario guide](../.claude/skills/scenario/SKILL.md).

[The independent-trials scenario](../scenarios/two_encounters_resolve_independently.ron)
pins activation, the final allowed kill, independent timeout and physical passage
through the opened door. The playable scene has both
[a completion scenario](../scenarios/soul_trial_can_be_completed.ron) and
[a timeout scenario](../scenarios/soul_trial_times_out_without_defeats.ron).

## Extending a rule

An ordinary encounter is data. A genuinely new objective needs a typed variant,
its narrowly scoped evaluation, shared observation vocabulary and a focused
scenario. Keep physical facts in sim and gameplay relationships in game. There
is deliberately no arbitrary expression evaluator, callback registry or script
language. Add those only if concrete compositions require them.

Runtime storage participates in scene eviction, fresh-run replacement, cached
restart hashes, deterministic hashes, reports and transition traces. Keep those
boundaries exhaustive when adding state. Simulation provenance belongs to the
health membership and pending spawn request; completed-tick outcomes are bounded
by the maximum body population and replaced every engine tick.
