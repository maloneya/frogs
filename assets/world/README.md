# Default world assets

`ground.glb` is a unit square on the XZ plane with a 128-cell checker texture.
Presentation scales it to the simulation's arena extent: the current 192-metre
arena keeps 1.5-metre checker cells, drawn as one mesh instance.

`prop.glb` is a bevelled octagonal plinth, 0.5 metres high and 0.5 metres across,
with its origin at ground level. Its white material accepts the presentation
layer's blue tint, turning green when the authoritative interaction state is
Activated. All static props currently use this asset and share one instanced
draw. Its geometry has no effect on collision, which remains the existing disc.

Regenerate both files with Blender:

```sh
blender --background --python assets/world/generate.py
```

The script is the editable source. It applies transforms and exports selected
geometry through Blender's GLB exporter, using the same validated import path
as the static preview. Runtime paths resolve from the build-time repository
location. Missing or invalid required assets fail startup explicitly.

`state.world_assets` exposes imported paths, geometry/texture counts and live
prop placements. `state.render.static_mesh_instances` and `static_mesh_draws`
include ground, props, and any optional static preview.
[Attack effects](../../docs/attack-effects.md) show committed attack reach;
[collision debug drawing](../../docs/collision-debug.md) exposes physical shapes.
