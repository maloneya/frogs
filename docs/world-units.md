# World units

The engine's unit contract lives in the [core crate documentation](../crates/core/src/lib.rs).
One world unit means one metre. This assigns a meaning to the existing numbers;
it does not rescale the world or change combat behaviour.

## Reading a size

The renderer starts with a cube one unit across, centred on the origin. A mesh
scale of 0.5 makes each side half a metre long. Nonuniform scale gives different
width, height and depth. Scale itself is a multiplier; a future imported mesh
must also have a known size before scaling tells us its final dimensions.

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

Today this is a documented contract. Floats and vectors do not carry units, so
the compiler cannot detect a distance accidentally supplied in centimetres or
pixels. A type alias or a constant equal to one would not provide that protection
either. Stronger enforcement would require distinct distance, position and speed
types at the APIs that accept those quantities, with explicit conversions at
rendering and import boundaries. Such types can prevent incompatible quantities
being mixed; they cannot detect an author choosing the wrong numeric value.

A future metre grid, reference cube or distance ruler would make the convention
visible during playtests. Those are presentation aids; their dimensions should
come from the same world measurements the simulation uses.
