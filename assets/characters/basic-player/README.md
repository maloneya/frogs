# Swordsman player

This retained sword foundation uses the CC0 Blender Human Base Mesh realistic male surface,
uniformly normalized to two metres and fitted to a 54-joint rig. A charcoal
hooded cloak covers the head and shoulders, with an open front, folded skirt,
tunic, leggings and leather boots. The body and slender longsword share one
mesh/atlas; clothing is a second skinned mesh with its own 256-pixel sRGB atlas.
Blender is Z-up / -Y forward; runtime is Y-up / +Z forward.
Anatomical right is negative X in both spaces. Bone names, skin weights and the
sword's `RightHand` → `RightGrip` attachment follow that physical side.

[Source geometry, licences and motion provenance](reference/README.md) remain
editable inputs. Shoulder weights remain refinement work.

The playable default is the [Thornbound sword glaive](../../weapons/thornbound-staff/README.md),
compiled from this foundation. This directory retains the reusable body, rig and
sword motion sources; regenerating it does not replace the equipped default.

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

Run [`review.py`](review.py) with **Python and Pillow**. In Codex, use the Python
executable returned by `load_workspace_dependencies`. The launcher checks image
dependencies before starting Blender, waits for rendering to finish, then packages
the results. It never regenerates or saves an input asset. Choose a new output
directory for each checkpoint; existing review work is refused.

Use `--before` to compare another `.blend` with `--after`, or omit both to inspect
the current player. Labels **A/B** are neutral identifiers in command order;
actual input paths and hashes stay in `evidence.json`, outside `critic/`.
This example produces an equipped and skeleton motion comparison from both the
game angle and side, with matched framing across assets, modes and all samples:

```sh
python3 assets/characters/basic-player/review.py target/player-review \
  --before /absolute/path/to/previous.blend --after target/player-a/basic-player.blend \
  --clips AttackCleave --views game side --modes equipped skeleton
```

`critic/` contains normal-speed and quarter-speed GIFs, timestamped pose strips,
and a local `index.html`. Give the independent critic selected images from this
directory using the [visual critique protocol](../../../.claude/skills/character-workflow/references/visual-critique.md).
`--references image.png ...` adds neutrally named copies of visual targets.
The evidence records input/tool hashes, camera framing, sample times and output
hashes. Packaging refuses changed inputs, missing/changed frames and existing
packets. A `.blend` preview establishes the authored appearance; it does not
establish that a separately exported GLB matches it.

Choose the evidence for the defect under review:

| Option | Use |
|---|---|
| `--views game front side opposite-side back` | Fixed full-body cameras with a two-metre floor grid; specify only needed views. |
| `--modes equipped plain skeleton` | Equipped character, plain body without clothing/sword, or bone diagram. Skeleton colours follow bone names and do not establish surface contact. |
| `--samples poses` | Inexpensive semantic poses: ready, loaded, release end, recovery and ready for melee; quarter-cycle samples for other clips. Stills only. |
| `--frames 13 14.75 19` | Exact or fractional Blender frames, labelled with presentation time. Stills only. |
| `--views grip-palm grip-back --hand right` | Opposing close-ups that follow the selected hand. Requires equipped mode and hand/finger landmarks. Use `--hand left` for the supporting hand. |
| `--grip-orbit 45` | Rotate both grip cameras around the hand axis when the hood or another limb occludes contact. |
| `--grip-cutaway` | Labelled hand/forearm/weapon isolation in grip views. Intact full-body views stay unchanged; use those for body/clothing clearance. |
| `--resolution 640` | Increase diagnostic image resolution (default 420). |

For clothing proportions or a sparse blocking pass:

```sh
python3 assets/characters/basic-player/review.py target/player-poses \
  --clips AttackCleave --views front side back --modes equipped plain --samples poses
```

For grasp, wrist and supporting-hand contact:

```sh
python3 assets/characters/basic-player/review.py target/player-grip \
  --clips AttackSlam --views front grip-palm grip-back --hand left \
  --grip-cutaway --frames 13 14.75 19
```

The complete motion is sampled by default. Pose strips include gameplay phase
edges, not just evenly spaced thumbnails. Sparse stills never become a GIF;
they cannot certify timing or feel. Grip cameras follow the hand, so use the
fixed full-body views to judge body travel and support. These are Blender
previews; use [ARPG_HARNESS](../../../.claude/skills/playtest/SKILL.md) for actual
game shading, contacts, transitions and standing/moving abilities.

Native references use the same renderer at their original scene cadence:

```sh
python3 assets/characters/basic-player/review.py target/slam-source-review \
  --source assets/characters/basic-player/reference/slam-source.glb --clips Sword_Regular_C
```

Native reference previews normalize height and preserve anatomical handedness.
Quaternius hand rigs
receive a diagnostic sword. Retimed player motion is a separate visual judgement.
Follow [motion acceptance](../../../docs/character-motion.md) and run the
[completion checks](../../../docs/verification.md) when promoting an asset.

[`preview.py`](preview.py) is the headless Blender renderer; [`preview_gifs.py`](preview_gifs.py)
packages an already completed render directory when these stages need to run
separately. [`preview_timing.py`](preview_timing.py) owns presentation-time sampling
and GIF cadence. Focused review-pipeline checks (same Python with Pillow):

```sh
python3 -B -m unittest discover -s assets/characters/basic-player -p test_preview.py
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/check_preview.py
```

Grasp regressions, including refusal of an open hand and wrist-mounted weapon:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python assets/characters/basic-player/check_grasp.py -- target/player-a/basic-player.blend
```
