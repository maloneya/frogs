# Basic enemy character

The horde asset is a short, hunched low-poly creature
built from boxes, joined into one mesh, and rigidly weighted to a seven-joint
armature. It carries only the two clips the horde presentation consumes:
`Idle` and `Run`. Presentation retimes `Run` into walking, running and sprinting
cadences according to actual movement, sharing two phase buckets per cadence.

Regenerate both checked-in artifacts from the repository root:

```sh
blender --background --python assets/characters/basic-enemy/generate.py
```

Open `basic-enemy.blend` to inspect the asset; edit the generator and regenerate
both artifacts for durable changes. The model faces
Blender `-Y`, uses metres and Z-up, and keeps its mesh object transform applied.
The broad shoulders, long arms, pale horns, and bright eyes are deliberately
readable at the game's pulled-out camera distance.
