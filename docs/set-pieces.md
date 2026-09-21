# Scene set pieces

A set piece is a named, finite timeline owned by one scene instance. It starts
only through `Game::trigger_set_piece(scene, name)`. There is no automatic
weather cycle, condition evaluator, scripting language or arbitrary property
mutation. The first supported action is a fading distant-light pulse.

## Try the storm reveal

Run `cargo run`, open F2 and select **Storm: reveal the horde**. Stand in the
single torch pool and press **F4**. A cold flash reveals 65 stationary enemies
beyond the torch's range, then fades to darkness over 15 fixed ticks (250 ms).
Press F4 again to restart the moment. Judge whether the glimpse gives enough
time to register the crowd before it disappears.

[The playable scene](../scenes/storm_reveal.ron) is the editable example.
F4 triggers only when exactly one set piece is installed across all live scenes;
menus consume the shortcut. For explicit selection, the harness accepts:

```text
scene start scenes/storm_reveal.ron
scene list
set-piece trigger c0 reveal_horde
state
trace since 0
```

Use the scene ID returned by the current run, rather than retaining `c0` across
starts or restarts. An absent name or evicted scene is refused without mutation.

## Ownership and timing

[Game's set-piece types](../crates/game/src/set_piece.rs) own the schema,
validation, schedule and observations. Content decodes these same types.
Each cue has a fixed tick offset and a typed action. Cues are ordered by offset;
equal offsets retain authored order. Multiple light pulses can overlap and add.

A trigger sets the timeline to `Queued`. The set-piece pass runs before the
engine tick, consumes the request and publishes offset zero. Each later game
tick advances one offset, independently of rendering frequency. A cue starting
at offset A with duration D contributes at offsets A through A+D-1, with
brightness `peak * (1 - (offset - A) / D)`. At A+D it contributes nothing.
The timeline returns to `Inactive` when its final action ends.

Retriggering queues a restart of the same timeline; it does not stack another
copy. A queued timeline contributes no light until the next step. The pass only
receives set-piece storage, never mutable Game or World. Presentation reads
active light values and cannot advance the timeline.

Names are unique within an instance. Loading the same scene twice creates two
independent timelines. Eviction removes its timelines and any queued/active
contributions; cached restart restores the authored definitions as inactive.
Complete admission validates names and effect capacity before physical mutation.
Definitions, pending triggers and elapsed state participate in the complete Game
hash; the physical engine hash remains independent of them.

Capacity is reserved for every authored pulse across live scenes, including
pulses that do not overlap. This conservative first implementation guarantees
that any combination of explicit triggers fits the renderer's shared directional
light limit. Admission refuses overflow; GPU upload also refuses excess input.
Timelines have a bounded duration, enforced by their validated constructors and
file decoding, with no unbounded per-tick allocation.

## Light and observations

[Core's directional light](../crates/core/src/lighting.rs) defines the direction
**toward** a distant source, linear RGB colour, intensity, and diffuse fill.
Fill blends between surface-facing illumination and uniform illumination. It is
an artistic approximation, not bounced light or visibility. Both static and
skinned surfaces sum these contributions with [torch light](lighting.md) in
linear colour. No active pulse means zero additional illumination.

Scene ownership does not impose a spatial boundary: these distant lights reach
all rendered world surfaces. There are no cast shadows or roof/wall occlusion.
UI, flames and attack-effect geometry retain their unlit presentation.

`state.sim.set_pieces` reports scene/name, phase and duration, plus the current
offset while running. `state.sim.directional_lights` reports the active output.
The separate set-piece trace records starts/restarts and completion at exact
ticks; diagnostics do not drive behavior or participate in hashes.

[The scene scenario](../scenarios/a_set_piece_reveals_the_horde.ron) checks the
same playable file, predicted intermediate brightness, retriggering, eviction
and replay. Game tests cover overlapping cues, independent instances, refused
loads, pending work, stale identities and restart. GPU tests check distant
illumination, facing/fill, linear fading, additive torches and clearing to black.
