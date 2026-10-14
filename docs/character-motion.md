# Character motion: visual acceptance

Start from a concrete authored reference and preserve whole-body motion through
retargeting. The current player's [compiler and previews](../assets/characters/basic-player/README.md)
own the shipped human body and motion. [Source provenance](../assets/characters/basic-player/reference/README.md)
records licences, clips and adaptations. The [reference catalog](../experimental/character-references/README.md)
retains additional bodies and motions for future work.

Review neck, waist and arm/torso negative space before equipment. Fit joints
inside the anatomy and inspect shoulder, hip and knee deformation under the
actual motion. Shoulder weights remain refinement work;
valid source data does not certify finished deformation.

## Principles to carry into other motions

- **Pose the whole action.** Show readiness, loaded effort, release and residual
  momentum with distinct whole-body silhouettes. Hips, chest, head, supporting
  leg and free arm belong to the same action. A blade path alone is insufficient.
- **Make support visible.** Lower or shift the centre of mass, bend the supporting
  knee, and show which foot takes the force. The reference's large compression
  works for this heavy slash; its exact depth is not a universal tuning target.
- **Preserve anatomical articulation.** Shoulder, upper arm, forearm and hand
  change relative to each other. The opposite arm can brace and then release or
  counterbalance on a different path. Independence does not require both hands
  to remain apart throughout every action.
- **Keep the grip solid.** Wrist motion is hand rotation relative to the forearm.
  The sword follows that hand. A separate weapon attachment is useful, but
  rotating the sword inside a fixed hand is not a substitute for a working wrist.
- **Let the body carry momentum.** The shoulder and arm lead the cut, while the
  hand and blade continue through its arc. Recovery contains residual motion;
  freezing the entire pose removes that information. Cloth follows the body.
- **Use timing contrast.** Load quickly enough to acknowledge input, accelerate
  the release, and settle after it. Locate the actual source strike before
  retiming; evenly speeding up the whole clip can put recovery inside contact.
- **Judge at the actual camera and speed.** Inspect clean silhouettes and
  articulation on an unarmoured mannequin first. Then inspect the equipped
  model through the game renderer at normal cadence, stationary and moving.
  Slow playback diagnoses problems but cannot certify feel.

## Working method

1. Select a concrete source clip and retain its provenance. Review native speed,
   slow playback and a pose strip before generating a replacement.
2. Normalize scale, facing and handedness explicitly. Inspect a loaded and a
   released pose after retargeting. A mathematically valid quaternion can still
   mirror the wrong motion or turn a forward lean into a backward arch.
3. Preserve source joint rotations and weight transfer. Add narrowly scoped
   adaptations for the game's aim, hit window, ready pose and floor. Avoid
   rebuilding the motion from isolated hand positions.
4. Match the existing gameplay phase schedule. Test the visible strike during
   the actual active ticks and intermediate interpolation values. Keep game
   damage, aim commitment, movement and hashes independent of presentation.
5. Review model deformation at shoulder, elbow, wrist, hip and knee. Armour must
   follow the body without hiding useful negative space or collapsing joints.
6. Keep approved renders and an editable source. Protect observable properties
   such as phase alignment, front-facing reach, torso movement and attachment
   continuity with checks. Passing structural tests does not establish beauty.

The current compiler lives in
[`authored_cleave.py`](../assets/characters/basic-player/authored_cleave.py).
It retargets the approved CC0 descending slash into the existing 54-joint player,
keeps the reference recovery moving, and preserves the 6/6/10 gameplay schedule.
Stationary attacks use the whole bracing pose; moving attacks retain directional
leg locomotion under the attack's articulated upper body. The shared importer,
mask and phase contract remain in
[character asset authoring](character-assets-authoring.md).

The accepted Slam is compiled by
[`authored_slam.py`](../assets/characters/basic-player/authored_slam.py) from the
CC0 `Sword_Regular_C` overhead segment. Its airborne, one-handed source is
adapted into a planted two-handed strike on the same production rig, retaining
compression, wrist motion and moving recovery. The 9/5/16 tick schedule remains
unchanged. Quarter-frame keys and continuous quaternion signs keep both grips
consistent between authored poses; Blender motion checks also inspect
intermediate samples. The user accepted this motion checkpoint on 2026-10-12.

Directional locomotion uses the compatible Quaternius humanoid jog for arm
counter-swing and KayKit's authored backward/strafe support on the same human
rig. Strafing hips turn toward travel while the head stays on mouse aim. The
compiler moderates torso lean and twist and preserves a rigid sword grip. Moving
ability layers compensate at the top masked joint for the locomotion parent's
rotation: hip articulation must not rotate committed attacks away from aim.
See [`authored_locomotion.py`](../assets/characters/basic-player/authored_locomotion.py)
and the [preview commands](../assets/characters/basic-player/README.md#review-motion).

The relaxed idle adapts the retained Quaternius `Sword_Idle` into a 3.2-second
loop on the same rig. Both arms rest lower with soft elbows; relative torso,
pelvis and wrist motion supplies a small weight shift while the feet stay planted.
Melee recovery meets its opening pose. The long blade rests slightly above
horizontal to preserve the Cleave wrist release; locomotion carries it higher
for stride clearance. This is a visual checkpoint awaiting user judgement of feel,
with compilation, grip/support and in-game presentation checked separately.
