# Scene lighting

Each scene owns an `environment` and an optional list of stationary `torches`.
The environment supplies ambient fill and one directional light; torches add
local pools of illumination. Adding or removing a torch never changes the
environment. Colours are linear RGB, with sRGB encoding only at display output.

An omitted environment uses neutral daylight: white ambient fill at 0.22 and
white directional light at 0.35, from direction `(0.4, 1.0, 0.3)`. Author a dark
scene explicitly by setting both intensities to zero, as in
[Jar of Souls](../scenes/soul_trial.ron). Direction points **toward** the light
in world X/Y/Z; magnitude does not affect brightness. It must remain finite and
nonzero even while the directional intensity is zero.

```ron
environment: (
    ambient_colour: (0.3, 0.4, 0.6), ambient_intensity: 0.15,
    directional_colour: (1.0, 0.8, 0.6), directional_intensity: 0.4,
    direction: (0.4, 1.0, 0.3),
),
```

[Game's EnvironmentSpec](../crates/game/src/environment.rs) owns the authored
schema and wraps [core's validated EnvironmentLight](../crates/core/src/lighting.rs).
Typed construction and file decoding share validation. The renderer receives
only this immutable value. **Save & play** applies a scene's authored environment;
editing a draft does not change the running game's lighting.

The most recently installed live scene selects the active environment. Evicting
it restores the preceding live scene's environment; evicting an older scene
leaves the active one unchanged. An empty game uses neutral daylight. This is
one global environment, with no spatial blending between additive scenes.
Restart restores the complete cached scene. All live scene environments and the
restart environment participate in the Game hash, while the engine-only
physical hash remains independent of lighting. `state.sim.environment` reports
the active settings and owner; `state.sim.scene_environments` reports every
live scene's settings so a later eviction is predictable.

Each torch supplies `position` (illumination centre, X/Y/Z in metres), linear RGB
`colour`, `intensity`, and `range` in metres. It is presented as a grounded
[campfire](../assets/world/woodland/README.md), with a stone ring, logs and a flame.
Light height controls illumination independently of the grounded mesh.
[Game's TorchSpec](../crates/game/src/torch.rs) wraps core's validated PointLight
and requires ground clearance. The shared `MAX_POINT_LIGHTS` permits eight lights
across all live scenes. Scene admission and GPU upload reject overflow; the
shader's array capacity is generated from this same constant.

Game owns torches in scene-installation and authored order. Additive loading
checks total capacity before installing physical content or gameplay relationships.
Refused loads leave live state, pending work, and the restart snapshot unchanged.
Eviction removes just that instance's torches. Torches have no collider or
tick-time behaviour. They affect the complete Game hash and restart fingerprint.
Scene ID and authored index identify each torch within a run.
`state.sim.torch_count` and `state.sim.torches` report their authoritative settings
and ownership. The app rebuilds its lights, campfires and flames from Game's
read-only observations. `state.world_assets.torch_instances` counts campfire meshes;
render effect counts include flames and attack effects.

Static and skinned surfaces share [one fragment-lighting function](../crates/gfx/src/lighting.wgsl).
Each pixel adds ambient fill, directional illumination scaled by surface facing,
and every point-light contribution before sRGB encoding. Point lights use a
positive surface-facing term, softened inverse-square distance falloff, and a
squared fade to black at their range. Per-pixel calculation lets even the
four-vertex floor show smooth pools between distant corners. Cost grows with the
number of point lights; their cap is a bounded starting point, not a performance
guarantee.

There is no simulated bounced light, shadow map, flicker, or bloom. Walls and
characters do not block light. Flames, attack effects, and UI are unlit geometry
and do not themselves illuminate surfaces. The remaining playtest question is
how much darkness feels good while moving and fighting.

[Scene-lifetime scenario](../scenarios/torches_follow_scene_lifetime.ron) checks
intermediate torch installation and eviction states. Game tests cover environment
ownership, refused loads, pending work, stale identities, hashes and restart.
GPU tests verify independent environment and point-light contributions, directional
facing, linear colour addition, all eight slots, and clearing the list without
changing the environment.
