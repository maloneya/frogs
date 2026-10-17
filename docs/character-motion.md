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

## Production checkpoints

The [player foundation](../assets/characters/basic-player/README.md#authored-motion)
owns the current Cleave, sword Slam, directional locomotion and relaxed idle
adaptations, with editable source and review commands. Its
[source README](../assets/characters/basic-player/reference/README.md) owns
licences and native clip provenance. Keep compiler details there rather than
maintaining another motion catalog here.

The user's five-pose Cleave checkpoint from 2026-10-15 remains the editable
foundation; the subsequent rear-foot drive and weight-transfer refinement awaits
judgement of feel. The grounded sword Slam was accepted on 2026-10-12.
Directional locomotion and idle remain visual checkpoints whose structural
checks do not establish finished deformation or feel.

The playable default is the
[Thornbound sword glaive](../assets/weapons/thornbound-staff/README.md), compiled
from that body and rig. Its compiler adapts the ready guard and replaces the
sword Slam with a one-handed lift and butt-first ground plant. The staff Slam
and [magic wave](attack-effects.md) await user judgement of feel; the retained
weapon playtest record predates those changes. The same gameplay phase schedule
remains authoritative.

The [wolf README](../assets/characters/wolf/README.md) owns its anatomy, source
motion adaptations, clip mapping and review commands. Review its native sources
and retimed production clips as separate evidence.
