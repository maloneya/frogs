# Woodland props

`generate.py` authors four flat-shaded static meshes in metres, with ground-level
origins, Blender Z up and -Y forward. Each exports one mesh, one opaque material
with backface culling, and one packed 64 × 8 RGBA palette texture. No rig or clips.

- `campfire`: roughly 1.9 m across, below 0.6 m tall; stone ring, ash and crossed logs.
- `pine`: 4.35 m tall, three evergreen tiers.
- `broadleaf`: roughly 4.5 m tall, clustered leafy canopy.
- `bare`: roughly 3.7 m tall, branching leafless trunk.

Regenerate editable `.blend` and runtime `.glb` files with Blender 5.2.1 LTS:

```sh
blender --background --factory-startup --python-exit-code 1 --python assets/world/woodland/generate.py
```

Add `-- /tmp/arpg-woodland` to export in isolation. Two clean builds must produce
identical GLBs. The asset importer tests enforce opaque, grounded, bounded meshes.

The app renders one campfire per scene light, replacing the old plinth-and-pole
stand. Flame geometry is unlit presentation above the logs; the scene's light
settings still determine illumination. See [lighting](../../../docs/lighting.md).

Scenes place trees through the game-owned `TreeSpec` in `trees`:

```ron
trees: [(kind: Pine, position: (12.0, 3.0), yaw: 0.5, scale: 1.0)],
```

Position is X/Z metres, yaw is radians, and scale is uniform. Game validates
placement, scale and the shared `MAX_TREES` capacity before scene admission.
Trees participate in scene eviction, restart, hashing and reports. Each tree
installs a small box matching its trunk base through the shared static collision
system. Kind, scale and yaw determine the footprint; the canopy is visual only.
Players, enemies, knockback and attack obstruction all use that same geometry.
Tree colliders share the engine wall capacity and are admitted atomically with
the rest of the scene. Their scene-local collider indices appear in tree reports. App batches each kind in
one draw. `state.sim.trees` exposes ownership and placements;
`state.world_assets.trees` reports imported meshes and per-kind instance counts.
The Soul trial bakes a random scatter (seed 73129) into 96 authored placements:
24 near the trial and 72 across the arena. Trees are at least 3.5 m apart, with
the central objectives and source spawn discs left clear. Coordinates are saved
in the scene, so load and restart never reroll the layout.
