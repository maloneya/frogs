# Thornbound traveler staff

The **sword glaive** is the playable default (B in the final two-way comparison,
earlier study E). The **large talon** is retained as the alternate (comparison A,
earlier study D). Both use the same helix wood, smooth grip, bronze binding and
casting eye from the supplied [design reference](reference/design.jpg).

| Blade | Standalone source/export | Equipped player | Overall height | Blade height / width |
| --- | --- | --- | --- | --- |
| Sword glaive | [workspace](variants/sword-glaive/thornbound-staff.blend), [GLB](variants/sword-glaive/thornbound-staff.glb) | [workspace](equipped/sword-glaive/player.blend), [GLB](equipped/sword-glaive/player.glb) | 2.53 m | 70 / 8 cm |
| Large talon | [workspace](variants/large-talon/thornbound-staff.blend), [GLB](variants/large-talon/thornbound-staff.glb) | [workspace](equipped/large-talon/player.blend), [GLB](equipped/large-talon/player.glb) | 2.34 m | 51 / 12 cm |

The standalone files have one mesh/material and a packed 2048 × 1024 RGBA albedo;
24,914 triangles for the glaive and 24,610 for the talon. Metres, Blender Z-up,
blade face toward -Y; GLB converts to Y-up. The origin is the primary grasp,
the uninterrupted grip spans ±0.225 m, and the supporting grasp is +0.15 m toward
the blade. Runtime uses lit base colour without a separate metallic response.
The [shared asset contract](../../../docs/character-assets-authoring.md) owns limits.

## Compile and check

`generate.py` authors the weapon; `equip.py` fits it to the retained
[basic player foundation](../../characters/basic-player/README.md). The equipped
export keeps its body, cloak, rig and eight role clips. Cleave/casting retain
the source action; idle holds the shaft vertically beside the body with a low,
closed right-hand grasp. Cleave and Slam enter and recover to that upright guard,
with floor clearance derived from the actual weapon geometry. Slam raises the primary right hand above the hood, drives the
butt down in front of the body and settles into grounded knee/torso compression.
The left arm opens to the side for balance and stays clear of the shaft. The first active sample
plants the butt; the remaining active samples hold it while the force wave
spreads. Eighth-frame keys preserve the fast stroke's grasp between samples.
`BeamOrigin` follows the physical blade tip. Combat damage, timing and
reach use the current gameplay values for both blades.

From the repository root, with Blender 5.2.1 LTS:

```sh
blender -b --factory-startup --python-exit-code 1 --python assets/weapons/thornbound-staff/generate.py -- target/glaive-source --blade sword-glaive
blender -b --factory-startup --python-exit-code 1 --python assets/weapons/thornbound-staff/equip.py -- target/glaive-player --blade sword-glaive
blender -b --factory-startup --python-exit-code 1 --python assets/weapons/thornbound-staff/check_equipment.py
cargo test -p arpg-assets --test thornbound_staff
```

Use `--blade large-talon` for the alternate. Both compilers require an explicit
output directory and default to the sword glaive. `equip.py` reads the retained
standalone workspace/landmarks from `variants/<blade>/`; after a deliberate
weapon edit, promote the generated `.blend`, `.glb` and `build.json` there before
compiling its equipped player. Promote both equipped outputs and their build
record only after checks pass. Two isolated builds of every retained static and
equipped asset must produce identical GLBs.

The compiler checks anatomical right-hand contact,
forearm clearance, ready seams, sampled weapon/body triangle surface crossings
and floor clearance. The lowest sampled staff vertex is 1.49 cm above the floor
for both blades. These samples do not certify every runtime blend or human feel.
`review.py` imports the standalone GLB afresh with matched framing; pass
`--views blade-front` for symmetry. Use the foundation's [motion review tools](../../characters/basic-player/README.md#review-motion)
for equipped normal/slow playback, semantic poses and opposing grip close-ups.

## Playtest

Ordinary startup and `character clear` select the sword glaive. In an
[ARPG_HARNESS](../../../.claude/skills/playtest/SKILL.md) session:

```text
scene start scenes/combat_feel.ron
character show assets/weapons/thornbound-staff/equipped/large-talon/player.glb
character clear
```

Send one command per socket connection; use absolute paths if the app runs from
a different directory. Scene restart restores targets and retains the selected
asset. Controls are 1/LMB (Cleave), 2/RMB (Slam), held Space (casting), and WASD.

The [playtest record](playtest.json) retains hashes and compact outcomes for the
12 stationary/moving combat cases under their historical D/E labels. Melee took
the three front targets from 3 to 1 health; casting damaged targets and ended on
release. Refused loads preserved the selection. Sampled review found no clear
high-impact grip/body defect; moving-plant bracing, Cleave recovery and impact
weight remain human feel questions. That record predates the raised-hand Slam
and magical force wave; it does not certify their timing or appearance. The
recorded sparse live benchmark is partial performance coverage.
