# Rectangular walls

Scenes author `engine.walls` using the simulation-owned
[`Wall`](../crates/sim/src/wall.rs) definition:

```ron
walls: [
    (centre: (0.0, 2.0), size: (6.0, 0.4)),
    (centre: (5.5, 5.0), size: (4.0, 0.3), yaw: 0.7853982),
],
```

Coordinates are world X/Z in metres. Size is the full local X/Z footprint.
Yaw is radians about Y, matching mesh instances: positive rotation turns +Z
toward +X. The validated constructor and file decoder require finite values,
at least one centimetre on both dimensions, and all corners inside the arena.
The owning module defines the shared live wall limit. Admission includes walls
from all resident scenes and rejects the complete load before changing state.

Walls are static rectangles, separate from circular player/enemy/prop storage.
They have no health, mass, seeking, interaction or height. Their stable identity
is `(SceneId, authored index)`. Installing a scene creates its walls; eviction
removes them together, and restart rebuilds the cached scene. Clearing enemies
preserves walls. The hash includes authoritative geometry and ownership;
`state.sim.scenes.walls` reports the same rectangles under `cN/index` keys.
Doors currently retain their existing circular prop geometry; they do not bind
wall rectangles in this slice.

## Motion

The wall pass sweeps the entire circular body's displacement, including powered
walking and carried momentum, against the rectangle. It tests flat faces and
rounded corner contacts, preventing fast movement from crossing a thin wall
between ticks. Contact removes the velocity component into the wall and retains
the component along it. There is no bounce or wall-impact damage.

Wall resolution runs before body contacts and again after body separation and
arena containment, so a crowd correction cannot push a body through a wall.
The trace aggregates `wall contacts` once per resolving pass. Small existing
body overlaps can remain when a crowded pile presses against an immovable wall;
wall exclusion takes priority over fully separating the pile in that tick.

Bodies initially overlapping a wall are pushed toward the nearest available
face inside the arena. Sweeps also include arena faces, so sliding cannot redirect
a body out of bounds.
Contact work per sweep is bounded; after eight contacts, unspent movement is
discarded. Do not author bodies inside solid wall intersections or passages
narrower than their diameter: those layouts may have no valid local placement.
The current implementation scans the bounded wall list with an inexpensive
bounding-box rejection. It has no wall spatial index yet.

## Attacks

Cleave, Slam and enemy melee require an unobstructed segment from attacker centre
to target centre in addition to their existing reach test. A shielded target
receives neither damage nor attack momentum. This deliberately makes a binary
cover decision for each target; partial body exposure does not grant a hit.

Disintegrate ends at the first wall on its centre ray and still checks cover for
each potential target, so its width/end cap cannot damage through the wall.
The app uses that same clipped endpoint and blocking-face normal to trim the
full band width, including its glow, at diagonal walls. Beam visuals use the completed tick's
pose; interpolating an old angle with a new clipped length could draw through
cover. Melee animation, fading effects and debug attack discs still describe
attack motion/potential reach; they are not solid geometry clipped by walls.

## Presentation and scope

The shared [wall asset](../assets/world/wall/README.md) supplies height and
appearance. App scales its unit footprint to the authoritative rectangle and
uses its exact yaw. The simulation never reads the mesh. Existing F3 body and
attack outlines remain circular; wall footprints are available in the report.
Lighting/shadows and enemy route planning are unchanged. Seekers can pile up
against a wall until the player goes around it.

Choose **Walls — movement and attack cover** in F2. Walk toward the long wall,
slide along it, try all three attacks, and go around its right end. The nearby
enemy behind it should remain shielded. The rotated wall tests diagonal sliding.
The same file is exercised by the wall scenarios in `scenarios/`; geometry,
scene lifetime, refusal, crowd pressure and attack controls also run through
Game tests. Automated checks establish physical rules; crowd feel and readability
remain playtest judgments.
