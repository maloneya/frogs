# World units

The engine's unit contract lives in the [core crate documentation](../crates/core/src/lib.rs).
One world unit means one metre.

## Reading a size

Imported meshes are authored in metres. A mesh scale of 0.5 halves those
authored dimensions; nonuniform scale adjusts each axis separately. Scale is
a multiplier, so the mesh must have a known size before scaling tells us its
final dimensions.

World positions use the same ruler. Two positions differing only by 3 on X are
three metres apart. X and Z describe the floor; Y describes height. The camera
decides how that distance projects onto pixels. Changing zoom changes the
picture, not the distance.

## Bodies and attacks

Visible geometry, physical collision and attack coverage have independent
dimensions. A narrow visible body can have a wider collision shape if that
makes physical contact feel better. All three still use metres.

An attack disc has a centre offset from the player and a radius around that
centre. For an illustrative disc centred one metre straight ahead with a
half-metre radius, its front edge is 1.5 metres ahead of the player's centre.
That is not the maximum distance to an enemy's centre: the enemy has its own
radius, and a hit occurs when the shapes overlap.

Actual tuning values stay beside the systems that own them: body dimensions
in [the world](../crates/sim/src/lib.rs), attack shapes in
[attack profiles](../crates/sim/src/attack.rs), and player speed in
[the walk pass](../crates/sim/src/pass/walk.rs). This guide does not duplicate
that catalog. Floor tile spacing is content too; a tile need not be one metre.

## Authoring and enforcement

Use metres for spatial values in scene files and harness commands, and interpret
spatial state fields as metres. Convert assets authored in centimetres at the
import boundary. Metres are a measurement convention, not a requirement for
realistic character proportions, movement speeds or weapon reach.

The [scene editor](scene-editor.md#placement-and-map) displays a metre grid,
world-axis compass and exact coordinates. Camera projection changes the visible
grid shape; snapping still uses world X/Z. Wall/tree inspectors show rotation
in degrees, while scene files store yaw in radians.

Floats/vectors do not carry unit types: validation bounds values but cannot detect
an author entering centimetres or pixels as metres. Keep conversions at the
input/import boundary.
