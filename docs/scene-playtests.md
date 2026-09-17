# Scene playtests

The first use is repeatable fresh playtests: switch configurations without
relaunching the game. A scene describes disposable content; a playtest owns the
world and player. Restarting constructs a fresh world, then calls the same
scene instantiation door that additive loading uses.

The app and scenario runner now own a `Game`, which wraps the engine world.
They step and observe it through the same entry point. Game owns the cached
restart description and atomically replaces the complete run on start/restart.
Game-owned relationships and engine resources share one eviction boundary. Additive loads
and eviction preserve the restart choice, even if its live instance is removed.
Run-scoped identity rules stay the same.

The `sim` report keeps its physical fields and adds `restart.available`, plus
`restart.name` and `restart.initial_hash` when a snapshot is selected. The latter
is the hexadecimal fingerprint of the snapshot's initial engine and gameplay state, computed
once when the snapshot is validated. It is immutable, so it cannot drift from
the description. The existing `sim_hash` and ready-reply hash now cover both the
active engine, gameplay relationships, and the restart effect. Their values intentionally change at this
migration; gameplay timing and event traces do not. `Game::engine_hash` remains
available for comparing engine state independently of the restart choice.

## Driving it

Launch from the repository:

```sh
. "$HOME/.cargo/env" && ARPG_HARNESS=/tmp/arpg.sock cargo run --release
```

Send one command per connection:

```sh
echo 'scene start scenes/activate_horde.ron' | nc -U /tmp/arpg.sock
echo 'scene start scenes/seekers.ron' | nc -U /tmp/arpg.sock
echo 'scene restart' | nc -U /tmp/arpg.sock
echo 'state' | nc -U /tmp/arpg.sock
```

`scene start <path>` reads the file and starts a fresh playtest. `scene restart`
reuses the selected parsed snapshot, so editing a file does not silently change
a repeated trial. Start the path again to pick up edits. Paths are relative to
the game's working directory, or absolute; the entire remainder is the path,
so spaces need no quoting inside the command itself.

Both reply `ready` only after the world is installed at tick zero, with its
initial hash and a new run id. The game continues running after the reply.
State reports the selected label, run id, current hash, and actual scene
membership. Trace replies also identify the run. Entity, source, and scene ids
are scoped to that run: discard previously read ids after restarting.

Additive lifecycle controls exercise the underlying mechanism independently:

```sh
echo 'scene add scenes/three_enemy_spawners.ron' | nc -U /tmp/arpg.sock
echo 'scene list' | nc -U /tmp/arpg.sock
echo 'scene evict c1' | nc -U /tmp/arpg.sock
```

Use the id returned by `scene add` or `scene list`. Adding the same file twice
creates independent instances. Eviction removes that instance's bodies,
sources, descendants, and queued emissions. It preserves the player and other
content. Restart removes additive instances as well as the previous player.

Boot uses the established horde grid, constructed in code as `Scene::boot` and
installed by the same loader. There is no scene environment variable or
working-directory-dependent boot fallback.

## In-game picker

`F2` opens the scene picker. W/S or Up/Down navigate, Enter starts fresh and closes, and
`F2` / `Esc` closes. `F1` switches to attack tuning. The world keeps running
while either panel captures gameplay input.

The first two choices are **Restart current (cached snapshot)** and **Default
horde**, followed by sorted `.ron` regular files directly inside `scenes/`,
relative to the working directory. The directory is scanned each time the
picker opens. Files are read only on selection, so choosing a file again picks
up edits; restarting the current snapshot does not. Broken files stay visible.
A missing directory still leaves both built-in choices available.

Starting uses exactly the harness's fresh-start boundary. Success closes the
panel; a read, parse, or validation failure keeps it open with an error and
preserves the current run. Repair the file and press Enter to retry. Long labels
and errors are shortened on screen; the full error is in `state.ui`. The list
scrolls to keep the selected row visible and adapts to the window height.

The picker is also drivable with `tap f2`, `tap up`, `tap down`, and `tap enter`.
`state.ui` reports whether it is open, its entries, the selected index, and any
error. App tests compare picker starts with direct file loads at tick zero,
exercise failure and retry, and verify modal input. HUD tests assert scrolling
and text bounds without a GPU.

## Content and execution

### Playable catalog

F2 offers **Default horde** (1,024 stationary enemies) and three files:

| File | Encounter |
|---|---|
| `seekers.ron` | Three approaching seekers |
| `activate_horde.ron` | An interaction starts a source that replenishes up to eight seekers |
| `three_enemy_spawners.ron` | Three approach directions build toward twelve seekers |

F1 opens the attack picker. W/S or arrows highlight **Cleave**, the default
broad arc, or **Slam**, an expanding frontal hit with stronger knockback.
Enter confirms and closes; F1/Esc cancels. R highlights Cleave. Fresh starts and restarts also
restore Cleave. Separate regression scenarios assert each attack's timing,
reach, hit order, impulse, and subsequent movement.

The small mechanism fixtures live inline in `scenarios/`; they are not picker
entries. The former stationary-grid and spawn-flood stress scenes have been
removed from the playable catalog.

### Historical simulation measurement

#### Measured simulation limit, 2026-09-10

Release build on the development machine, no game window running. Each entry
is the best of three twelve-tick means measured **inside `World::step`** by the
scenario runner, using stationary grid populations. Loading, hashing, replay
comparison, and rendering are outside that timer.

| Enemies | Best mean tick | Share of 16.67 ms |
|---:|---:|---:|
| 1,024 | 0.15 ms | 1% |
| 4,096 | 2.21 ms | 13% |
| 8,192 | 8.84 ms | 53% |
| 10,240 | 13.84 ms | 83% |
| 11,264 | 16.84 ms | 101% |
| 12,288 | 19.87 ms | 119% |
| 16,384 | 35.50 ms | 213% |
| 32,768 | 142.56 ms | 855% |

The measured 60 Hz simulation boundary is between 10,240 and 11,264 bodies
for this workload. Rendering and a concentrated seeking crowd need additional
time, so this is an upper bound on a comfortable playable population, not an
FPS guarantee or a worst-tick measurement. Doubling population from 8,192 to
16,384 costs almost four times as much: the existing pair loop tests
`N * (N - 1) / 2` pairs even when there are no overlaps. A spatial broadphase
is the next system needed to change that growth rate.

To repeat the measurement with the existing runner, create a temporary
scenario outside `scenarios/` with `setup: (enemies: N)`, twelve budget ticks,
and `max_mean_step_micros: 0.001`. The deliberately impossible budget reports
the measured mean as a failure. Run three times with
`cargo run --release -p scenario -- /path/to/probe.ron` and take the lowest
reported mean. Confirm the only failure is the timing probe. Keep these probes
outside the correctness gate; change the budget to 16,666.67 to test 60 Hz.

### Scene format

```ron
(
    name: "Small encounter",
    bodies: [
        (pos: (0.0, 4.0)),
        (pos: (4.0, 4.0), what: (seeks: true)),
    ],
    grids: [
        (origin: (-6.0, 8.0), columns: 8, rows: 4, spacing: 0.7,
         what: (seeks: true)),
    ],
    sources: [
        (pos: (12.0, 0.0), radius: 3.0, every: 60,
         when: FewerThan(12), what: (seeks: true)),
    ],
)
```

Coordinates are world `(x, z)`. `Scene`, `Placed`, `BodyGrid`, `Template`, and
`SourceSpec` live in sim; the content library reads those types directly.
Source population conditions still count the whole world's enemies, not just
their scene.

A `BodyGrid` is shorthand for placements rather than a new kind of content. It
expands once at load, through the same door `bodies` uses, so no later pass can
tell the two apart. Authored `bodies` are placed first, then each grid in list
order; inside a grid, `columns` along +X vary fastest and `rows` step along +Z.
That order is the contract, because scenario `nth` assertions, eviction and the
determinism hash all address bodies by placement order.

Admission validates nonzero `rows` and `columns`, a positive finite `spacing`,
and runs the placement arithmetic on the far corner before a single body
exists — a bad field in a grid is thousands of bad bodies otherwise. Capacity is
charged from the dimensions, so an oversized grid is refused without being
expanded. A 128x128 horde can be described with one grid entry rather than
16,384 individual placements.

```text
scene file -> content library -> Scene description
                                     |
                    Game prepares the complete lifecycle operation
                                     |
                 +-------------------+------------------+
                 |                                      |
         fresh playtest                           additive load
         World::empty()                           existing World
                 |                                      |
                 +---------- World::load_scene ---------+
                                     |
                        ready scene instance + owned ids
                                     |
                   sources -> owned requests -> spawn grants
```

Instantiation is synchronous between ticks. Content validation and body
capacity checks happen before mutation. Already queued spawns retain capacity;
authored placements use the common placement implementation without squeezing
through the 256-request queue. Success means every placement and source is
installed. No hidden ticks run. Sources first evaluate on the next tick.

Fresh restart also resets player motion, attack state and tuning, pending UI
edits, accepted gameplay input, the accumulator, and camera follow/lead. Held
physical keys require release before acting again. Old delayed harness actions
and captures receive cancellation replies. Construction failure leaves the
current playtest untouched. Renderer settings such as zoom and vsync persist.

## Verification and next boundaries

The scenario runner accepts `setup.scenes` and timed `scenes` operations:

```ron
setup: (scenes: [File("../scenes/seekers.ron")]),
scenes: [
    (at: 2, action: Load(Inline((name: "another", bodies: [])))),
    (at: 4, action: Evict(1)),
],
```

References resolve relative to the scenario file, once before both replay runs.
Eviction, source, and control indices refer to load order, starting with setup,
then ascending tick order, preserving file order at equal ticks. Validation and
execution share this order. Placed body numbering includes authored bodies,
then ordinary setup actions, then later loads and emissions in execution order.
`scene_count` and existing body/source assertions inspect the result.

Step-time budgets now measure `Game::step`, including its call to the existing
engine schedule. Setup, hashing, and assertions remain outside the timer. The
historical measurements above predate this wrapper and measured `World::step`.

The lifecycle scenario asserts surviving identities and positions, source
descendants, reload, and load-then-evict before a source's first tick. Unit tests
cover capacity refusal, stale queued ownership, slot reuse, bulk reset, hashing,
and the app's complete restart boundary. CLI tests reject bad or unreachable
commands even with blessing enabled. Runtime placement tracking requires an
untruncated trace; a batch that overflows it fails explicitly rather than
renumbering the bodies an assertion refers to.

Public game lifecycle tests also cover failed replacement preserving its
snapshot and accepted pending work, restart after additive loading and eviction,
reset of interaction/motion/attack state, and hashing of distinct restart choices
when their active engine worlds are identical. App cleanup runs only after the
game operation succeeds; invalid content and exhausted run ids preserve the
current run without cancelling its inputs or capture requests.

These instances are disposable. An enemy following the player elsewhere still
belongs to its originating instance and disappears on eviction. Reload recreates
authored content. Persistence, ownership transfer, proximity residency, partial
activation, background file loading, and changing arena bounds remain separate
design work. Synchronous instantiation can hitch for large scenes. Source ids
still retain the existing registry's historical holes; long-lived streaming
will need to revisit that storage assumption.

## Gameplay scene composition

New gameplay scenes wrap physical content in `GameScene.engine` and add
`source_controls`. Legacy engine-only files still decode unchanged. Inline
scenario references use `Gameplay(...)` for complete game scenes and retain
`Inline(...)` for legacy physical scenes. File references support both forms.
See [source control](source-control.md) for validation and lifecycle rules;
`scenes/activate_horde.ron` is the playable connected scene; minimal fixtures
are inline in the regression scenarios.
