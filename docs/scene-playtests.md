# Scene playtests

The first use is repeatable fresh playtests: switch configurations without
relaunching the game. A scene describes disposable content; a playtest owns the
world and player. Restarting constructs a fresh world, then calls the same
scene instantiation door that additive loading uses.

## Driving it

Launch from the repository:

```sh
. "$HOME/.cargo/env" && ARPG_HARNESS=/tmp/arpg.sock cargo run --release
```

Send one command per connection:

```sh
echo 'scene start scenes/pair.ron' | nc -U /tmp/arpg.sock
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
echo 'scene add scenes/stream.ron' | nc -U /tmp/arpg.sock
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

`F2` opens the scene picker. `Up` / `Down` select, `Enter` starts fresh, and
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

```ron
(
    name: "Small encounter",
    bodies: [
        (pos: (0.0, 4.0)),
        (pos: (4.0, 4.0), what: (seeks: true)),
    ],
    sources: [
        (pos: (12.0, 0.0), radius: 3.0, every: 60,
         when: FewerThan(12), what: (seeks: true)),
    ],
)
```

Coordinates are world `(x, z)`. `Scene`, `Placed`, `Template`, and `SourceSpec`
live in sim; the scenario library reads those types directly. Source population
conditions still count the whole world's enemies, not just their scene.

```text
scene file -> scenario library -> Scene description
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
setup: (scenes: [File("../scenes/pair.ron")]),
scenes: [
    (at: 2, action: Load(Inline((name: "another", bodies: [])))),
    (at: 4, action: Evict(1)),
],
```

References resolve relative to the scenario file, once before both replay runs.
Eviction indices refer to load order, starting with setup. Operations run in file
order before the named tick. Placed body numbering includes authored bodies,
then ordinary setup actions, then later loads and emissions in execution order.
`scene_count` and existing body/source assertions inspect the result.

The lifecycle scenario asserts surviving identities and positions, source
descendants, reload, and load-then-evict before a source's first tick. Unit tests
cover capacity refusal, stale queued ownership, slot reuse, bulk reset, hashing,
and the app's complete restart boundary. CLI tests reject bad or unreachable
commands even with blessing enabled. Runtime placement tracking requires an
untruncated trace; a batch that overflows it fails explicitly rather than
renumbering the bodies an assertion refers to.

These instances are disposable. An enemy following the player elsewhere still
belongs to its originating instance and disappears on eviction. Reload recreates
authored content. Persistence, ownership transfer, proximity residency, partial
activation, background file loading, and changing arena bounds remain separate
design work. Synchronous instantiation can hitch for large scenes. Source ids
still retain the existing registry's historical holes; long-lived streaming
will need to revisit that storage assumption.
