# Scene lifetime and automation

Use the [scene editor](scene-editor.md) to build playable content. This page owns
its loading, restart and identity contracts. Both the app and headless scenario
runner drive `Game`; content extracts the same `GameScene` from the
[authoring document](scene-authoring.md#document-and-runtime-boundary) for either caller.

## Start, restart and startup

A successful start installs a fresh world/player at tick zero and caches the
validated scene as its restart snapshot. Restart rebuilds that snapshot with
current runtime tuning. It does not reread the file. Additive loads and eviction
leave the restart choice intact, even after evicting its live instance.

The app resets player motion, attacks/recovery, accepted input, accumulator,
feedback, audio and camera follow/lead only after replacement succeeds. Held
physical keys must be released before acting again. Pending harness actions,
trials and captures receive cancellation replies. Zoom, vsync and runtime tuning
persist. Failed construction or an exhausted run ID preserves the current run
and accepted pending work.

Interactive launches reread the last successfully played path from
`target/last-scene.json`. No preference, a missing file or invalid content leaves
an empty player world; errors appear in the companion. Opening or saving without
playing does not select a startup scene. This preference is app-local, outside
Game state and replay.

Harness launches neither restore nor overwrite it. Set `ARPG_SESSION_FILE` to
an isolated path when explicitly testing restoration.

## Harness commands

Launch with the [live playtest guide](../.claude/skills/playtest/SKILL.md), then send
one command per connection:

```sh
echo 'scene start scenes/soul_trial.ron' | nc -U /tmp/arpg.sock
echo 'scene restart' | nc -U /tmp/arpg.sock
echo 'scene add scenes/three_enemy_spawners.ron' | nc -U /tmp/arpg.sock
echo 'scene list' | nc -U /tmp/arpg.sock
echo 'scene evict c1' | nc -U /tmp/arpg.sock
```

Start reads the file; restart uses the cached description. Paths resolve from
the game's working directory unless absolute. Everything after `scene start`
or `scene add` is the path, including spaces. Start/restart reply `ready` with
the tick-zero hash and new run ID after installation; simulation then continues.
Discard previously read entity, source and scene IDs when the run changes.

Add installs an independent instance in the current world; use its returned ID
or `scene list` for eviction. Eviction removes its bodies, sources, descendants,
queued emissions and game-owned resources, preserving the player and other
instances. An enemy remains owned by its originating instance wherever it moves.
Restart clears additive instances too. Reload recreates authored content; this
is not save-game persistence or streaming residency.

## Content and admission

[GameScene](../crates/game/src/scene.rs) wraps physical
[Scene](../crates/sim/src/scene.rs) content under `engine` and adds trees,
[lighting](lighting.md), [encounters, doors and chests](encounters.md), and enemy
attack relationships. The content crate decodes these owning types directly.

```ron
(
    scene: (
        engine: (
            name: "Small encounter",
            bodies: [(pos: (0.0, 4.0), what: (seeks: true))],
            grids: [(origin: (-6.0, 8.0), columns: 8, rows: 4, spacing: 0.7,
                     what: (seeks: true))],
            sources: [(pos: (12.0, 0.0), radius: 3.0, every: 60,
                       when: FewerThan(12), what: (seeks: true))],
        ),
    ),
)
```

Coordinates are world `(x, z)` in metres. Source population conditions count
all world enemies. `BodyGrid` expands once through ordinary placement: explicit
bodies first, then grids in list order, columns along +X fastest and rows along
+Z. Placement order matters to scenario `nth` bindings and deterministic hashes.
Grid validation checks nonzero dimensions, finite positive spacing, far-corner
arithmetic and capacity before expansion. Use grids for regular crowds instead
of thousands of generated body entries.

Instantiation is synchronous between ticks. Complete validation precedes mutation;
accepted queued spawns retain their reserved capacity. Authored placements bypass
the bounded spawn-request queue but use the same placement implementation.
Success installs every placement and source without hidden ticks; sources first
evaluate on the next tick. Large loads can therefore hitch. Long-lived streaming
would also need to address historical source-registry holes.

## Observation and tests

State reports the selected name, run ID, active hash and live membership; trace
replies identify their run. `sim.restart` exposes `available`, and when selected,
`name` and `content_hash`. That fingerprint describes the cached scene instantiated
with built-in tuning; current tuning is hashed separately. `sim_hash` covers
active engine/game state and the restart effect; `Game::engine_hash` isolates
physical state. All owned resources participate in eviction, restart and reports.

Scenarios own their scene setups and can add/evict test instances on a timeline:

```ron
setup: (scenes: [(engine: (name: "Test seeker", bodies: [(pos: (0.0, 4.0), what: (seeks: true))]))]),
scenes: [
    (at: 2, action: Load((engine: (name: "another", bodies: [])))),
    (at: 4, action: Evict(1)),
],
```

External scene references are not supported; playable files remain free to change.
Scene indices follow setup order, then ascending tick and authored order within
a tick. Placed-body numbering includes setup scenes, setup actions, then later
loads/emissions in execution order. Stable spawn tickets and completed-tick
outcomes preserve bindings independently of diagnostic trace retention.

Use the [scenario guide](../.claude/skills/scenario/SKILL.md) for assertions and
timing. The scenario timeline has no restart operation.