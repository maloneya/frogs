# Felled trees

Every tree is a body on a locked [topple joint](joints.md): its root is the
lock, and holds against ground-plane load the way a [door latch](physical-doors.md) does. Once it fails, the tree falls
about its base under gravity, strikes what it comes down on and lies on the
ground as a solid log.

Open [the felled grove](../scenes/felled_grove.ron) in Scenes and **Save & play**.
A crowd presses on the pine that plugs a gap in the wall. When the root fails,
the pine falls onto a second pine, and its momentum breaks that root too.
Restart to compare standing aside, being struck and slamming the small bare trees.

[Joints, locks and impacts](../crates/sim/src/joint.rs) belong to simulation
and know nothing of trees. [Trees](../crates/game/src/tree.rs) derive each
trunk's topple joint and root lock from the authored kind and scale, choose the
contact height, and decide damage. The [shared solver](../crates/sim/src/solver.rs)
loads roots through contacts; [the topple pass](../crates/sim/src/pass/topple.rs)
tips, stops and lands them.

## Height in a planar simulation

Bodies, walls and contacts live on the ground plane. Trees need two conventions:

- Every ground-plane load acts on a trunk at its 1 m contact height.
- Bodies are that tall when a falling trunk comes down on them, and they give
  no vertical support.

Walls, door panels and standing trunks count as at least as tall as any tree.
Fallen logs do not, and a falling trunk lands across them.

## The root

While rooted, a trunk's square base is immovable to the solver. Contacts with
bodies and door panels, and direct impulses such as attacks, load the root. The
load is a vector: crowds pushing from opposite sides cancel. As with a latch,
`root_impulse` is a threshold over one fixed tick, and below-threshold loading
never accumulates. Once a tick's load exceeds it, the root fails having spent
exactly its strength. The remainder, `|load| - root_impulse`, is the first
tipping impulse in the load's direction. A root that breaks right at its
threshold therefore starts slowly, near the unstable upright balance. A larger
excess starts the fall faster.

Strength comes from kind and scale; no rule counts enemies or exempts the
player. At unit scale a pine holds 160 kg m/s, a broadleaf 220 and a bare tree
120. Strength scales with trunk cross-section (scale²) and mass with volume
(scale³). With default tuning, Slam (72) cannot fell a unit-scale tree but does
fell a bare tree at scale 0.6. A blocked enemy of 40 kg and 30 m/s² supplies
20 kg m/s per tick, so about eight pushers' combined load fails a pine. The
first arrival of a moving chain is an impact, briefly above its steady load.

## The fall

A falling trunk is a slender rod turning about its base toward its fall
direction, with angular acceleration `1.5 g sin(tilt) / height`. Mass sets its
inertia, `mass * height² / 3`, so it determines how much an impulse tips it.
It does not change how fast gravity alone brings it down. The fall direction is
fixed once the root fails, and the trunk cannot tip back past upright.

While the contact height still stands over the base, `tan(tilt) <= width / 2`,
contacts on the base keep acting at that height. Bodies behind it push it over
and bodies in front hold it back. After that, the pivot takes them.

The base square stays solid during the fall. At its height a trunk moves
horizontally at `rate * contact_height`, at every point along its length. It
meets each body once per fall: when it has come down to the contact height over
the body's nearest edge, within the crown half-width of the fall line. That
meeting is an inelastic exchange along the fall between the body's mass and the
trunk's rotation, so the trunk slows by what it gives. Fixed bodies are met but
take no impulse. As with attacks, a struck body moves from the next physics step.

A fall that would carry the span beyond the base into a wall, panel, standing
trunk or the arena edge stops there, leaning. The angular momentum it loses goes
into a door or tree as an impulse along the fall at the contact height. Each tick
gravity renews that, so a leaning tree keeps loading a door latch or a
neighbour's root. A trunk falling into a standing tree usually breaks that root
too. If the obstacle leaves, the trunk falls again.

At a right angle the trunk lands and its collider becomes a log: trunk width,
from half a base behind the pivot to the tip. A log is static, like a wall, and
blocks movement, attacks, beams and interaction cover. Crowds cannot push it.

## Damage

The engine records each meeting as an `Impact` with the closing speed and
delivered impulse. Gameplay damages a struck body by 3 when the trunk closed
on it at 1 m/s or more. Health alone decides eligibility, so the player,
enemies and anything else with health are hurt. Damage is applied after the
player's attack and before actor effects. The trunk is the damage's actor in the
`Damaged` event.

## Observation and lifetime

`Game::tree_state` reports Standing, Falling, Down or Missing; `Game::joint`
exposes lock, direction, tilt as its angle, rate, root reaction and collider.
The engine trace records `LockBroken` and `Toppled`. Reports include
`state.sim.joints`, `state.sim.impacts` and each tree's body and state.
Trunk bodies follow a scene's authored bodies, so authored indices are
unchanged; formations are placed after them.
`MAX_TREES` is gameplay's share of the simulation's joint capacity. Eviction removes trunks mid-fall, and restart rebuilds
standing trees.

Presentation pitches the tree mesh by the interpolated tilt about its base.
Instances carry only yaw and pitch, so when the root fails, a falling tree
turns to face its fall: the canopy turns once about the trunk.
