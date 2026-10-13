# Scene lighting

Use **Scene lighting** and **Campfire** in the [editor](scene-editor.md), or the
[headless API](scene-authoring.md). A scene's `environment` supplies ambient fill
and one directional light; `torches` add local illumination. Adding a torch never
changes the environment. **Save & play** applies draft changes to the game.

Omitting the environment uses neutral daylight. For darkness, set both intensities
to zero. Direction points **toward** the light in world X/Y/Z; its magnitude does
not affect brightness, and it must be nonzero even at zero intensity. Colours
are linear RGB. Rendering inputs use the shared
[colour types](../crates/core/src/colour.rs); encoded sRGB requires explicit decoding.

```ron
environment: (
    ambient_colour: (0.3, 0.4, 0.6), ambient_intensity: 0.15,
    directional_colour: (1.0, 0.8, 0.6), directional_intensity: 0.4,
    direction: (0.4, 1.0, 0.3),
),
```

[EnvironmentSpec](../crates/game/src/environment.rs) owns defaults and validation.
The newest live scene selects the global environment; evicting it restores the
preceding scene's. There is no spatial blending. An empty game uses daylight.
`state.sim.environment` reports the active owner/settings;
`state.sim.scene_environments` includes inactive live environments.

[TorchSpec](../crates/game/src/torch.rs) defines position, colour, intensity and
range. Position is the illumination centre in metres; height changes the light
independently of the grounded [campfire mesh](../assets/world/woodland/README.md).
All live scenes share core's `MAX_POINT_LIGHTS` budget. Torches have no collider.
`state.sim.torches` reports settings/ownership; `state.world_assets.torch_instances`
counts campfire meshes. Flames share the render effect counts with attack effects.

[The shader](../crates/gfx/src/lighting.wgsl) adds sky fill, directional and point
illumination in linear space before display encoding. Directional light and the
nearest campfire cast moving shadows from world meshes and animated characters.
The directional map locks its texel grid to the world so camera easing does not
make stationary edges shimmer. Fire shadow edges soften with distance; a small
world-space receiver offset keeps shadows close to their casters.
The horde can use a smaller [shadow mesh](character-assets-authoring.md) with the
same sampled pose. Campfire light flickers; translucent flame tongues and rising
embers are rebuilt from presentation time. Only the point light illuminates
surfaces; the flame geometry, attack effects and UI are unlit.

**Rain and lightning** (`environment.storm`) adds varied wind-driven rain streaks and brief ground impact rings, periodic
lightning that reveals the scene and its shadows, and wet surface highlights.
These effects never change gameplay. There is no bounced light or bloom.
