# World units

The [core crate](../crates/core/src/lib.rs) defines the unit convention: metres,
seconds and radians; X/Z span the floor and Y is height. Floats and vectors do
not enforce units. Convert external measurements at import boundaries.

Visible mesh dimensions, collision shapes and attack coverage are independent.
Mesh scale multiplies authored dimensions; camera zoom changes only projection.
An attack's reach also includes the target's collision radius when testing overlap.
Tuning stays with the [body](../crates/sim/src/lib.rs) and
[attack](../crates/sim/src/attack.rs) owners.

The [editor](scene-editor.md#placement-and-map) displays a metre grid and world-axis
compass. Snapping uses world X/Z despite camera projection. Rotation inspectors
use degrees; scene files store radians.
