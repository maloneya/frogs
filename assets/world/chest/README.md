# Reward chest

A 1.04 m wide wooden chest facing runtime +Z, with a 0.6 m high closed lid.
`closed.glb` and `open.glb` are static poses with one mesh, one material and
one embedded 8×8 palette PNG each. `chest.blend` retains both editable poses.
The open pose rotates the lid 105 degrees around its rear hinge. App selects
one pose from Game's authoritative chest state; there is no animation or loot.
Collision stays the engine's static disc, independent of the visible box.

Regenerate with Blender 5.2.1 LTS:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python assets/world/chest/generate.py -- /tmp/arpg-chest
```

The output includes both GLBs, the blend file, and fixed front/side/back/three-quarter
renders of both poses. Compare clean GLBs before copying the three assets here;
diagnostic PNGs stay outside the repository. Assets tests check import, dimensions
and raised-lid height. App tests check state selection and eviction.
