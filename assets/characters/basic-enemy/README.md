# Basic enemy character

The horde asset is a hunched low-poly creature with tapered, blended-weight
limbs, one mesh and a 19-joint armature. `Idle`, `Run`, `Hit` and `Death`
are the ordinary-horde presentation catalog. Its 0.95 runtime scale makes the
1.79 m mesh about 87% of player height, with a narrower torso and tucked arms. Presentation retimes `Run` into walking, running and sprinting
cadences according to actual movement, sharing two phase buckets per cadence.

Regenerate the visible character and shadow caster from the repository root:

```sh
blender --background --factory-startup --python-exit-code 1 --python assets/characters/basic-enemy/generate.py
```

Open `basic-enemy.blend` to inspect the asset; edit the generator and regenerate
their `.blend` and `.glb` artifacts for durable changes. The model faces
Blender `-Y`, uses metres and Z-up, and keeps its mesh object transform applied.
The broad shoulders, long arms, pale horns, and bright eyes are deliberately
readable at the game's pulled-out camera distance.

`basic-enemy.shadow.glb` is an invisible caster on the same 19-joint rig.
It retains the body silhouette, feet, claws and horns while omitting the belt,
eyes and mouth. The generator builds both assets from the same geometry catalog,
rig and actions. Runtime uses the visible animation palette. The caster's
editable source is `basic-enemy.shadow.blend`.
