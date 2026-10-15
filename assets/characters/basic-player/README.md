# Swordsman player

The default player uses the CC0 Blender Human Base Mesh realistic male surface,
uniformly normalized to two metres and fitted to a 54-joint rig. A charcoal
hooded cloak covers the head and shoulders, with an open front, folded skirt,
tunic, leggings and leather boots. The body and slender longsword share one
mesh/atlas; clothing is a second skinned mesh with its own 256-pixel sRGB atlas.
Blender is Z-up / -Y forward; runtime is Y-up / +Z forward.
Anatomical right is negative X in both spaces. Bone names, skin weights and the
sword's `RightHand` → `RightGrip` attachment follow that physical side.

[Source geometry, licences and motion provenance](reference/README.md) remain
editable inputs. Shoulder weights remain refinement work.

## Compile and check

From the repository root with Blender 5.2.1 LTS:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/generate.py -- target/player-a
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/generate.py -- target/player-b
cmp target/player-a/basic-player.glb target/player-b/basic-player.glb
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/check_motion.py -- target/player-a/basic-player.blend
python3 -m unittest discover -s assets/characters/basic-player/reference -p 'test_*.py'
```

Omit the output argument to regenerate the checked-in `.blend` and `.glb`.
[`generate.py`](generate.py) owns fitting, equipment, the low resting guard and Disintegrate.
[`anatomy.py`](anatomy.py) preserves the authored surface and smooths anatomical
weights along adjacency. Joining eyes and sword must preserve body vertices and
faces. [`motion.py`](motion.py) owns shared import cleanup, bind conversion and keys.

## Clothing source

[`clothing-source.blend`](clothing-source.blend) is the editable bind-space
clothing input. The compiler appends `HoodedCloak` and attaches the existing rig;
ordinary regeneration preserves source edits. The hood follows `Head`, the
shoulder wrap follows the upper torso, and the skirt blends down to `Pelvis`.
The chest wrap, shoulders and back are one welded cloth surface; its folds share
the same skin weights across the join. The fitted hood tapers into a pointed crown.
The front stays open for stride and sword clearance. Cloth movement is baked
skinning, without runtime cloth simulation or additional joints.

[`author_clothing.py`](author_clothing.py) records the initial panel construction,
fabric folds, shell thickness, sleeve fitting and packed woven texture. To
deliberately reconstruct that source (replacing later artistic source edits):

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/author_clothing.py
```

## Authored motion

- [`authored_idle.py`](authored_idle.py) adapts Quaternius' `Sword_Idle` into a
  3.2-second relaxed loop: low arms, small pelvis/torso weight transfer and
  independent wrist motion, with planted feet and a rigid sword grip. Attacks
  recover to its opening pose; running keeps an elevated blade for clearance.
- [`authored_cleave.py`](authored_cleave.py) compiles the edited five-pose
  [`cleave-poses.blend`](cleave-poses.blend), baking its hip and foot controls
  onto the original rig. The wind-up keeps moving; the release curves ease into
  the cut without continuing the wind-up after contact starts. A shorter rear
  stance loads the leg, then its heel releases as the hips carry forward into a
  planted front foot. Foot arcs clear the floor on the step back and return.
  Half-frame keys occupy the existing 6/6/10 tick schedule.
- [`authored_slam.py`](authored_slam.py) adapts an airborne one-handed finisher into
  a planted two-handed strike. Quarter-frame keys preserve the shared grip and
  follow-through in the 9/5/16 tick schedule.
- [`authored_locomotion.py`](authored_locomotion.py) combines humanoid arm swing
  with backward/strafe support. Hips turn 60 degrees toward sideways travel,
  chest twist is moderated and head yaw stays on aim. Four half-second cycles
  share a supporting-leg phase.

[`generate.py`](generate.py) authors a 1.48 m longsword for the two-metre
body: a 1.135 m blade, 5.6 cm wide at its root, with distal taper, a recessed
fuller on both faces, a compact steel guard and a leather grip. The lower grip
retains the Slam supporting-hand position; blade length retains its ground strike.
[`grasp.py`](grasp.py) owns the equipment relationship: anatomical finger-segment
boundaries define the hand rig and palm frame; handle radius drives finger closure.
One final pass aligns the weapon to `RightGrip` and derives supporting-hand reach
for Slam, after retargeting and before export. The compiler refuses failed skinned
contact or forearm clearance across clips and fractional samples. `Weapon` is a
fixed child of `RightGrip`; `BeamOrigin` follows its tip, 1.22 m from the grasp.
Player admission also validates the fixed hierarchy/closed pose and mesh contact
before GPU upload. Running, attacks and channel blends share that relationship.
Moving abilities retain leg locomotion and aim above the turning pelvis;
stationary attacks use the braced pose. The app owns role selection, phase
mapping and masks; see [the asset contract](../../../docs/character-assets-authoring.md).
Presentation never drives gameplay timing, aim or movement.

## Edit Cleave poses

[`cleave-poses.blend`](cleave-poses.blend) is the editable five-pose source, separate
from the generated player. Its labelled timeline and `HOW TO - hips and foot
controls` text explain the authoring controls. Move the hip ring to reposition
the body with feet held at their targets; move or rotate either foot rectangle
to place it. The optional knee guides live in their own hidden bone collection.
Record pose changes with **I**, then save the file. Ordinary character compilation
reads this authored input, validates its bind and ready seam, and samples the
evaluated poses onto the original deform rig. The extra controls and IK remain
in this file; the game receives ordinary transform animation.

[`add_pose_controls.py`](add_pose_controls.py) adds these authoring-only controls
to a separate output copy, matches the existing key poses, and checks planted
feet while raising the hips. It refuses an existing output or an already
controlled rig; ordinary character regeneration does not run it.

## Review motion

Keep a previous `.blend` and matching `.glb` in an ignored review directory.
One renderer compares clips at gameplay cadence with equal camera, lighting and
scale; omit `--before` for the current asset alone:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/preview.py -- target/player-review \
  --before /absolute/path/to/previous.blend --after target/player-a/basic-player.blend \
  --clips AttackCleave AttackSlam Run RunBackward RunLeft RunRight
python3 assets/characters/basic-player/preview_gifs.py target/player-review
```

Packaging requires Pillow and produces normal/quarter-speed GIFs and pose strips.
`evidence.json` records hashes, sampled frames and cadence; packaging refuses stale
inputs. These are Blender previews. Use [ARPG_HARNESS](../../../.claude/skills/playtest/SKILL.md)
for actual shading, contacts, transitions and standing/moving abilities.

Native references use the same renderer at their original scene cadence:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/preview.py -- target/slam-source-review \
  --source assets/characters/basic-player/reference/slam-source.glb --clips Sword_Regular_C
python3 assets/characters/basic-player/preview_gifs.py target/slam-source-review
```

Native reference previews normalize height and preserve anatomical handedness.
Quaternius hand rigs
receive a diagnostic sword. Retimed player motion is a separate visual judgement.
Follow [motion acceptance](../../../docs/character-motion.md) and run the
[completion checks](../../../docs/verification.md) when promoting an asset.

Grasp regressions, including refusal of an open hand and wrist-mounted weapon:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/check_grasp.py -- target/player-a/basic-player.blend
```
