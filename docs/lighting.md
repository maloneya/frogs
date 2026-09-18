# Scene lighting

Scenes can author stationary torches under `torches`, alongside `engine` and
`source_controls`. See [the two-torch scene](../scenes/two_torches.ron) for a
complete editable example. Run `cargo run`, open F2, and select **Two torches:
warm and cool**. Walk between their pools to see colours mix and brightness add.

Each torch supplies `position` (flame centre, X/Y/Z in metres), linear RGB
`colour`, `intensity`, and `range` in metres. The stand extends from the ground
to the flame. Omitted `torches` means no lights. Existing playtests explicitly
author their original warm torch; the built-in boot scene authors its own.

[Game's TorchSpec](../crates/game/src/torch.rs) owns the authored torch schema.
It wraps [core's validated PointLight](../crates/core/src/lighting.rs), shared
with the renderer, and requires ground clearance for the visible flame. File
decoding and typed construction use the same validation. The shared
`MAX_POINT_LIGHTS` currently permits eight lights **across all live scenes**.
The shader's array capacity is generated from this constant. Renderer upload
also refuses overflow rather than silently truncating the list.

Game owns torches in scene-installation and authored order. Additive loading
checks total capacity before installing any physical content or controls.
Refused loads leave live state, pending work, and the selected restart snapshot
unchanged. Eviction removes just that instance's torches; restart reconstructs
the cached scene, including its lights. Torches have no collider or tick-time
behaviour. They affect the complete Game hash and restart fingerprint, while the
engine-only physical hash stays independent of lighting. Scene ID and authored
index identify a torch within a run; no mutable dense index escapes storage.

`state.sim.torch_count` and `state.sim.torches` report authoritative light settings
and scene ownership, keyed by scene ID and authored index. The app rebuilds its lights, stands and flames from Game's
read-only observations. `state.world_assets.torch_instances` counts stand pieces;
render effect counts include flames and attack effects. Removing the final
scene light leaves the world dark, with no global fallback.

Both static and skinned surfaces use [one fragment-lighting function](../crates/gfx/src/lighting.wgsl).
Each pixel sums the lights' direct contributions in linear colour before sRGB
output encoding. Each contribution uses a positive surface-facing term,
softened inverse-square distance falloff, and a squared fade to black at the
light's range. Per-pixel calculation lets even the four-vertex floor show
smooth pools between its distant corners. Cost grows with the number of lights;
the cap is a bounded starting point, not a performance guarantee.

There is no ambient/bounced light, shadow map, flicker, or bloom. Walls and
characters do not block light. Faceted flames, attack effects, and UI are unlit
geometry and do not themselves illuminate surfaces. The remaining playtest
question is how much darkness feels good while moving and fighting.

[Scene-lifetime scenario](../scenarios/torches_follow_scene_lifetime.ron) loads
the same playable file and checks intermediate installation/eviction states.
Game tests cover refused loads, pending work, stale identities and restart;
GPU tests cover additive colour/brightness, all eight slots, and clearing the
light list to black.
