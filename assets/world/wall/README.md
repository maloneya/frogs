# Solid wall

`wall.glb` is an opaque rectangular box with a 1 × 1 metre ground footprint,
1.5 metre height, and a ground-centred origin. App scales local X/Z to each
simulation wall's size and applies the same yaw. Height is part of this asset;
there is no height field in simulation. One material and an embedded opaque
4 × 4 PNG follow the static-mesh import contract.

Regenerate with Blender (authored with 5.2.1 LTS):

```sh
blender --background --factory-startup --python-exit-code 1 --python assets/world/wall/generate.py
```

Pass `-- /tmp/arpg-wall` for an isolated build. The generator saves both the
editable `.blend` and the runtime `.glb`. Two clean isolated builds must produce
identical GLBs. Importer tests assert the footprint, height, grounding and opacity.
See [wall behaviour](../../../docs/walls.md) for authoring and playtesting.
