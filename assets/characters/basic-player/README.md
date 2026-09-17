# Basic player character

This is the first deliberately simple Blender-authored player: a two-metre,
low-poly adventurer built from boxes, joined into one mesh, rigidly weighted to
an eight-joint armature, and exported with the player presentation clip catalog.

Regenerate both checked-in artifacts from the repository root:

```sh
blender --background --python assets/characters/basic-player/generate.py
```

The generator is the walkthrough as executable source. Its sections follow the
authoring order: clear the scene, make and pack the colour atlas, build UV-mapped
parts, join the one mesh, create the armature, bind vertex groups, author named
actions, save the `.blend`, and export the `.glb`.

Open `basic-player.blend` to inspect or refine the source. The model faces
Blender `-Y`, uses metres and Z-up, and the mesh object's transform is applied.
The GLB exporter converts that to the engine's Y-up, `+Z`-forward convention.

The sword is part of the joined mesh and is rigidly weighted to `Weapon`. Its
local +Y axis points from the right hand toward Blender `-Y`, so the ordinary
character skinning draw keeps the grip and blade attached through every pose.

Every attack action uses frame 1 for neutral entry, frame 13 for the completed
wind-up, frame 19 for contact/follow-through, and frame 31 for recovery to
neutral. Keep the major striking motion between frames 13 and 19: app maps that
authored interval to the simulation's committed active ticks.

Cleave winds the torso left, carries the sword across the front from left to
right during frames 13–19, then recovers. The exported-asset presentation test
checks the direction throughout the active interval and rejects a path behind
the player. This first pass retains the eight-joint rig; elbow and wrist
articulation are a separate improvement. The broad damage disc remains a
simulation shape, not a literal outline of the blade.
