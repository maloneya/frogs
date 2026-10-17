# Default world assets

`ground.glb` is a unit square on the XZ plane with a muted dirt texture.
Presentation scales it to the simulation's 192-metre arena extent; its UVs
repeat the image 96 times, giving a two-metre tile in one mesh instance.
`ground-dirt.png` is the source image; the generator embeds a 256 × 256 copy
and saves an editable `ground.blend`. Runtime builds the mip chain in linear
colour space and samples through sRGB, as with other world materials.

The source image was generated with the built-in imagegen tool. Prompt:
“Seamless square repeating game ground base-colour texture; subtle muted dark
grey-brown dirt, fine dusty grain and soft mottling, restrained low contrast.
Orthographic overhead flat albedo, uniform illumination, no shadows or highlights,
no perspective. Matching opposite edges, no borders, objects, grass, large stones
or text. Small-scale detail for a two-metre patch; opaque PNG.”

`prop.glb` is a bevelled octagonal plinth, 0.5 metres high and 0.5 metres across,
with its origin at ground level. Its white material accepts the presentation
layer's blue tint, turning green when the authoritative interaction state is
Activated. Ordinary static props use this asset and share one instanced
draw. Its geometry has no effect on collision, which remains the existing disc.

Regenerate both files with Blender:

```sh
blender --background --factory-startup --python-exit-code 1 --python assets/world/generate.py
```

Pass `-- /tmp/arpg-world` to build into a separate directory.

The script and source PNG are the editable sources. It applies transforms and exports selected
geometry through Blender's GLB exporter. The validated static-mesh importer
loads both assets. Runtime lookup follows the
[shared asset contract](../../docs/character-assets-authoring.md): launch from the
directory containing `assets/`. Missing or invalid required assets fail startup
explicitly.

`state.world_assets` exposes imported paths, geometry/texture counts and live
prop placements. `state.render.static_mesh_instances` and `static_mesh_draws`
include ground, props and one campfire per live scene light and scene-owned trees.
[Lighting](../../docs/lighting.md) describes scene-authored torches and their shared limit.
[Attack effects](../../docs/attack-effects.md) show committed attack reach;
[collision debug drawing](../../docs/collision-debug.md) exposes physical shapes.

The [chest assets](chest/README.md) provide closed and open reward props.

The [wall asset](wall/README.md) is a unit-footprint rectangular box used for
scene-authored solid walls. Its height belongs to the asset, while app scales
the ground footprint from simulation geometry.

The [woodland assets](woodland/README.md) provide the campfire and three solid-trunk tree variants.
