# World units

Use metres, seconds, kilograms and radians. The [core crate](../crates/core/src/lib.rs)
defines the ground-plane convention: X/Z span the floor and Y is height. Floats
and vectors do not enforce units. Convert external measurements at import boundaries.

Visible mesh dimensions, collision shapes and attack coverage are independent.
Mesh scale multiplies authored dimensions; camera zoom changes only projection.
Attack overlap uses the target's physical shape: circle radii for ordinary
bodies and the current rectangle for hinged panels. Tuning stays with the
[body](../crates/sim/src/pass/spawn.rs), [motion](../crates/sim/src/pass/motion.rs)
and [attack](../crates/sim/src/attack.rs) owners.

An impulse is momentum change in kg m/s; for constant force, `J = F × Δt`.
Player input and autonomous movement both drive bounded motors, with target
speed in m/s and acceleration in m/s². Mass converts that acceleration to force
and determines the velocity change from an impulse. Overlap correction moves at
most at the push speed and leaves no momentum. [Door latch strength](physical-doors.md) is a
reaction impulse measured over one fixed simulation tick at the latch's lever
arm, not health damage accumulated over time.

Contact frequency is in Hz, damping ratio is dimensionless, and the overlap push
speed is in m/s. Overlap within a 0.5 mm slop is held rigidly. The shared impulse owner
also bounds resulting point speed to keep accepted state inside the solver's
numerical range, independently of mass, allegiance and ability.

The [editor](scene-editor.md#placement-and-map) displays a metre grid and world-axis
compass. Snapping uses world X/Z despite camera projection. Rotation inspectors
use degrees; scene files store radians.
