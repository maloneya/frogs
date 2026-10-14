# Physical doors

A door is a rectangular panel with mass, a fixed hinge, angular stops and a
breakable latch. Its body survives release and breaching. Passage clearance,
contact and cover follow the panel's actual angle; latch state alone does not
mean that a doorway is open.

Open [the crowd breach scene](../scenes/door_breach.ron) in Scenes and **Save & play**.
The player starts across a wall from two sources. Leave the gate intact and let
the crowd build; restart to compare pushing, attacks and changes to physical tuning.

A door is a body on a locked [hinge](joints.md). [Joint and lock
mechanics](../crates/sim/src/joint.rs) belong to simulation; [doors](../crates/game/src/door.rs)
own the authored panel format, convert it to a hinge and lock, and read the lock
as the door's latch. The [shared solver](../crates/sim/src/solver.rs) resolves body
contacts, rotating panels and their attachments together. The current assembly
keeps its hinge after latch failure; it does not fracture into fragments.

## Loading and failure

Player input and autonomous movement drive the same bounded motor mechanism.
Blocked enemies continue applying effort; a connected crowd transmits that load
through its contacts. Walls, spacing, body mass and placement determine how much
load reaches the latch. Enemy count is never a breach condition.

Circular bodies transmit pressure through the shared
[contact response](crowd-collision.md#contact-response). Warm starting carries
each contact's force from tick to tick, so a steady crowd delivers its combined
motor load. The hinge and angular stops are joint constraints; latch strength
alone determines when the attachment fails.

Attacks and contacts supply impulses in kg m/s. Impulses applied away from the
hinge also change angular momentum, so pushing near the free edge has more
leverage than pushing beside the hinge. The latch resists the angular motion
required to hold the panel closed. Its reported reaction is expressed as an
impulse at the edge opposite the hinge.

While latched, the panel is immovable to the solver. Its latch absorbs the
angular impulse that contacts and direct impulses deliver during the tick; a panel
takes no motor of its own.
`latch_impulse` is a threshold over one fixed simulation tick; below-threshold
loading does not accumulate as lifetime damage. Once a tick's absorbed load exceeds
it, the latch fails having spent exactly its strength, and the excess becomes the
panel's spin in the same substep. The breach therefore carries the momentum that
caused it, and a broken latch reports its strength as that tick's reaction.
Overlap correction is relaxed away within each substep, so it does not load the latch.

Neither the player nor any attack has a special exemption. A test scene can
choose a threshold that resists its tuned player impulses but yields to a packed
horde. Increasing the player's impulse or changing the contact arrangement can
change that outcome through the same mechanism. Health and gameplay allegiance
control damage eligibility, independently of physical impact.

The [tick schedule](../crates/sim/src/step.rs) applies motors before contact
resolution. Attacks read the resulting positions and submit impulses afterward;
those impulses affect displacement and latch reactions on the next physics step.

## Release, authoring and observation

An encounter's `UnlockDoor` releases the latch without adding motion. Releasing
and breaking are different attachment outcomes, and both retain collision and
cover until the panel moves. A breached door remains a valid encounter endpoint;
removing its body is a separate lifecycle event. Restart reconstructs the closed,
latched assembly from the accepted scene.

Use the Door preset in either [authoring client](scene-authoring.md). Placement
sets the closed centre; width and closed yaw determine the hinge along its local
negative-X edge. Author the opening between wall segments and leave physical
room for the swing. Inspector angles use degrees; files use radians. Shared
admission validates the body binding, dimensions, swing bounds and complete load
before mutation. Mass belongs to the body; attachment tuning belongs to the panel.

`Game::joint` exposes the hinge angle, rate, lock state and the latest reaction.
The engine trace records each breach as `LockBroken` on the tick it happens;
the encounter trace records `UnlockDoor` releases.
`Game::door_state` reports Latched, Released, Broken or Missing. Reports include
`state.sim.joints`; presentation consumes `Game::door_presentations` without
changing the simulation. Movement, attack overlap, beam clipping and interaction
cover use the same panel shape at its current pose.
