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

[The shader](../crates/gfx/src/lighting.wgsl) adds ambient, directional and point
illumination in linear space before display encoding. There are no shadows,
bounced light, flicker or bloom; walls and characters do not block light. Flames,
attack effects and UI are unlit and do not illuminate surfaces.
