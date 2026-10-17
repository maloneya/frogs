# Rectangular walls

Draw walls in the [scene editor](scene-editor.md#placement-and-map). Saved
`engine.walls` use the simulation-owned [`Wall`](../crates/sim/src/wall.rs):

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

Walls are static rectangles, separate from entity body storage.
They have no health, mass, autonomous movement, interaction or height. Their stable identity
is `(SceneId, authored index)`. Installing a scene creates its walls; eviction
removes them together, and restart rebuilds the cached scene. Clearing enemies
preserves walls. The hash includes authoritative geometry and ownership;
`state.sim.scenes.walls` reports the same rectangles under `cN/index` keys.
[Doors](physical-doors.md) and [tree trunks](felled-trees.md) are jointed
bodies; they reuse rectangle geometry while simulation owns their
[joints and locks](joints.md).

## Motion

The [shared contact solver](../crates/sim/src/solver.rs) constrains circular
bodies against walls with speculative contacts, so a body stops at the face
rather than entering it. Contact response removes closing velocity while
allowing tangential motion; there is no wall-impact damage. Bodies faster than
the solver's collision passes can follow are swept against faces and rounded
corners, so fast knockback cannot cross a thin wall.

Walls, crowd contacts and hinged panels participate in the same physical solve.
A body placed inside a wall is pushed out through the nearest face that keeps it
inside the arena, at no more than the overlap push speed and without adding
velocity. Dense or impossible layouts can retain small body overlaps. Do not
author bodies inside intersecting solids or passages narrower than their diameter.

The [rectangle owner](../crates/sim/src/wall.rs) supplies swept movement,
penetration and segment queries. It also keeps a per-cell index of nearby
walls, rebuilt when walls are installed or evicted, so body contact tests visit
only walls near each body.

## Attacks

Cleave, Slam and autonomous attacks test physical shape overlap and require an
unobstructed segment from the attack origin to the resulting contact point.
Static walls and every jointed body's current box (door panels, standing
trunks and logs) provide cover; the contacted body itself is excluded from its
own cover test. A shielded target receives neither damage
nor attack momentum. Damage eligibility is independent of physical contact.

Disintegrate ends at the first wall or jointed box on its centre ray and still checks cover for
each potential target, so its width/end cap cannot damage through the wall.
The app uses that same clipped endpoint and blocking-face normal to trim the
full band width, including its glow, at diagonal walls. Beam visuals use the completed tick's
pose; interpolating an old angle with a new clipped length could draw through
cover. Melee animation, fading effects and debug attack discs still describe
attack motion/potential reach; they are not solid geometry clipped by walls.

## Presentation and scope

The shared [wall asset](../assets/world/wall/README.md) supplies height and
appearance. App scales its unit footprint to the authoritative rectangle and
uses its exact yaw. The simulation never reads the mesh. Ordinary body and
attack-disc outlines remain circular; walls and panels have rectangular footprints.
Wall meshes cast [directional and nearest-fire shadows](lighting.md); the physical
rectangle supplies gameplay cover independently of those meshes. Pursuit actions
have no route planning, so actors can pile up against a wall.

For a playtest, draw a long wall and a rotated wall in a new scene and place an
enemy behind each. **Save & play**, walk and slide along the faces, then try all
three attacks. Shielded enemies should remain unharmed until you go around the
wall. Judge lighting and visible clearance in the game.