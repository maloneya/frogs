# Player source assets

`cleave-source.glb` contains the original rig, skin and four unmodified sword
animation payloads from Quaternius' **Universal Animation Library 2 Standard**,
authored with Gonzalo Furnier. The pack is CC0; see [LICENSE.txt](LICENSE.txt).
Original `UAL2_Standard.glb` SHA-256:
`8cee20ab1bc55130092447e810e26df22dd2803eccc54f52137a7d54d7ab88a8`.
Retained source SHA-256:
`7f0612ce1ae2776c62b2be632c4feb1e7fb7e40975fc9614a25301a307dafd13`.

The user approved the descending slash as the visual baseline on 2026-10-12.
`Sword_Regular_B` and `Sword_Regular_B_Rec` drive the player compiler.
`Sword_Regular_A` and its recovery remain for comparison. This is authoring input;
its 65-joint rig and creator clip names do not satisfy the runtime player contract.
The compiler retargets into the 26-joint player and eight app roles.

Production timing and handedness normalization belong to
[`authored_cleave.py`](../authored_cleave.py). The source's anatomical sword hand
is on negative X; the player's named sword arm is on positive X. Mirroring that
handedness preserves forward lean; rotating the motion by 180 degrees does not.
See [motion acceptance](../../../../docs/character-motion.md) before extending
this approach to other actions.

## Accepted body source

`realistic-male.blend` retains the realistic male body and its two eyes from
Blender Studio/community **Human Base Meshes v1.4.1**, plus the bundle README.
[Official source](https://www.blender.org/download/demo-files/#assets).
[Licence](ANATOMY-LICENSE.txt): CC0. The archive SHA-256 is
`811f43accbb31a88266d932f8f5563b2d13586fca0ba2693aad1f5fe582b3515`.
The retained file SHA-256 is
`574d06727d1e6306eda670738978e9d8e35e5987a2f6ee152f5f70a6879fa520`.

Extraction retains all 10,582 body base vertices, polygon indices and anatomical
face sets. It removes the multires authoring modifier/cache. The source surface
is uniformly scaled and grounded by `anatomy.py`; it is not procedurally rebuilt.
The production compiler owns fitting and skinning. Extract from the original
bundle with [extract_body.py](../../../../experimental/character-references/extract_body.py),
passing `GEO-body_male_realistic` as the body name. The [player preview tools](../README.md#review-motion)
and [source gallery](../../../../experimental/character-references/README.md)
provide motion and anatomy inspection.

## Slam source

`slam-source.glb` retains `Sword_Regular_C` from the same Quaternius / Gonzalo
Furnier CC0 Standard pack, without resampling its rig, skin or curves. Its SHA-256
is `9093cc940d1f4500b01baab924525f441e86122a593016d8db0efaee70ea49a9`.
The original pack hash is recorded above. Extract with
`python3 assets/characters/basic-player/reference/extract_clips.py <UAL2_Standard.glb>
<slam-source.glb> Sword_Regular_C`.

The source's one-handed, airborne combo finisher is a motion reference, not an
accepted Slam by itself. The user accepted the grounded adaptation on
2026-10-12. `authored_slam.py` selects its overhead segment at 0.625–0.700 s,
retimes it into the existing 9/5/16 tick schedule, plants the feet, narrows torso
rotation, fits both arms to a shared grip, and steepens the downward blade arc.
Those are explicit adaptations; native source playback remains distinct from
the retimed player preview. See the [preview commands](../README.md#review-motion).

## Directional locomotion sources

`jog-source.glb` retains Quaternius Universal Animation Library Standard's
`Jog_Fwd_Loop`, CC0, with unmodified skin and curve payloads. Original UAL1 GLB
SHA-256: `69591853d817488edaa8fd9bf8fc1d821eaeaf789f8627b3cd23b41c4ed67997`.
Retained input: `08c54fdd7db54ce51c9e403b18df65dfb8e0a7db7df30f51f5e37fe23cbb8bb8`.

`locomotion-source.glb` retains Kay Lousberg's KayKit Adventurers 1.0 Knight
`Running_Strafe_Left`, `Running_Strafe_Right`, and
`Walking_Backwards`. Original Knight SHA-256:
`60428e3abc09ba83e595d256e3af8c5c976b46cdae599f0802fc82b4a3445168`.
Retained input: `be2223068bfde1872c00cb4587f34256cbf35f3877dc27e8351b3cd611a0fa5b`.
The original is pinned in the [reference catalog](../../../../experimental/character-references/catalog.json).
[KayKit licence](KAYKIT-LICENSE.txt): CC0. [extract_clips.py](extract_clips.py) retains the
original meshes, skin and chosen animation bytes without resampling.

`authored_locomotion.py` combines the humanoid jog's torso/clavicles and opposite
arm swing with KayKit's backward/sideways support. Forward motion uses the
humanoid jog throughout. It mirrors handedness, retargets bind axes, grounds the
feet, moderates torso lean, keeps chest twist within 12 degrees and head yaw on
aim, and carries the sword through a fixed wrist attachment. All four cycles
retain the existing half-second gait duration and common supporting-leg phase.
These adaptations are authoring decisions, distinct from native source playback.
