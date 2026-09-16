# Basic enemy character

This is the first dedicated horde asset: a short, hunched low-poly creature
built from boxes, joined into one mesh, and rigidly weighted to a seven-joint
armature. It carries only the two clips the horde presentation consumes:
`Idle` and `Run`.

Regenerate both checked-in artifacts from the repository root:

```sh
blender --background --python assets/characters/basic-enemy/generate.py
```

Open `basic-enemy.blend` to inspect or refine the source. The model faces
Blender `-Y`, uses metres and Z-up, and keeps its mesh object transform applied.
The broad shoulders, long arms, pale horns, and bright eyes are deliberately
readable at the game's pulled-out camera distance.
