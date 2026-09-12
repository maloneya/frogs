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

The `Weapon` bone is intentionally unweighted. Its local +Y axis points from the
right hand toward Blender `-Y`; the app uses that animated joint transform to
place its engine-rendered weapon.
