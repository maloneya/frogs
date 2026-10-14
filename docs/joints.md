# Joints and locks

Simulation knows mechanisms, not world objects. A jointed body moves along one
coordinate, and an optional lock holds that coordinate until a tick's load breaks
it. Gameplay assembles world objects from these parts and decides what their
states mean. [Doors](physical-doors.md) are a body on a locked hinge;
[trees](felled-trees.md) are a body on a locked topple joint.

[Joint configuration, state and impacts](../crates/sim/src/joint.rs) belong to
simulation. The [shared solver](../crates/sim/src/solver.rs) resolves contacts
with every jointed body's current box. [The topple pass](../crates/sim/src/pass/topple.rs)
moves poles out of the ground plane.

## Parts

- **Hinge.** A box turning about a vertical axis at its local negative-X edge,
  between angular stops. It moves in the ground plane, so contacts turn it
  directly once its lock is gone.
- **Topple joint.** A pole standing on a square base. Once unlocked, it tips
  about a horizontal axis at its base under gravity until it lies flat.
  - The ground plane has no height, so the pole declares a contact height. Ground-plane contacts act on it there, and bodies count as that tall when it comes down on them.
  - It also declares a sweep: the half-width of the swath in which it meets bodies.
  - Contacts on its base keep tipping it only while the contact height still stands over the base.
- **Lock.** A threshold on the load the joint absorbs in one fixed tick,
  measured as linear impulse at the joint's arm. The arm is a hinge's free
  edge, or a pole's contact height. While held, the body is immovable; contacts
  and direct impulses load the lock instead of moving the body. Loading never
  accumulates across ticks. Once it is exceeded, the lock fails having spent
  exactly its strength. The excess becomes the joint's motion in that substep:
  a hinge's spin, or a pole's tipping load and fall direction. `release_lock`
  removes a held lock without adding motion of its own; impulses admitted
  earlier that tick still act. A pole released without load stands balanced
  until a load chooses its direction. Impulse admission bounds a pole without a
  direction as if the whole impulse tipped it.
- **Impact.** A lossless record of a moving jointed body meeting another body
  during the latest tick: closing speed and delivered momentum. Gameplay may
  turn an impact into damage through `ResolvedStep::damage_impact`, once per
  impact, attributed to the moving body.

Walls, hinges and standing poles block a pole's fall as if tall. A pole that
lies flat does not. A blocked fall delivers the momentum it loses to a jointed
blocker at the contact height.

## Building a new jointed object

1. **Scene binding.** Bind a joint to an explicit dynamic, non-character scene
   body with a `JointSpec`. Simulation admission validates the binding,
   uniqueness, motion bounds and the shared `MAX_JOINTS` capacity atomically.
2. **Authored format.** Keep it in gameplay, as `DoorPanel` and `TreeSpec` do,
   and convert it to a `JointSpec`. The file format then stays independent of
   physical vocabulary.
3. **Observation.** Derive the object's state from `JointState`: lock state,
   angle, `grounded`, collider and reaction. Don't keep a second copy.
4. **Gameplay rules.** Damage, rewards and presentation choices stay in
   gameplay; consume impacts there.

Trace events are `LockBroken` and `Toppled`. Reports include `state.sim.joints`
and `state.sim.impacts`. The collision outline view draws every joint's current box.

## Not yet supported

Jointed bodies have fixed pivots: there are no free rigid boxes. A pole lying
flat is a static log that crowds cannot push. Heights other than a pole's
contact height do not exist.
