# Attack effects

Cleave leaves a blue crescent; Slam produces an amber expanding ring. Both use
committed active hitbox samples from `Game::attack_discs`. Changing the selected
attack mid-swing does not change the effect. Cleave's trail stays in world space;
Slam follows its live hitbox, then leaves a fading final ring. Fading never extends
the damage window. Start/restart clears history.

[App effect code](../crates/app/src/attack_effects.rs) owns geometry, interpolation and visual tuning.
The renderer draws blended triangles after opaque geometry, with depth testing
but no depth writes. Visual interpolation does not become collision geometry.

`state.attack_effects` reports observed tick, active state, retained samples,
committed profile/centre/radius and vertex count. `state.render.effect_vertices`
and `effect_draws` report rendering workload.

## Contact flash

Surviving enemies flash white when their observed health drops. Feedback begins
when damage can first be rendered; catch-up ticks collapse to the latest health.
Repeated redraws do not refresh it. Defeated enemies leave no flash image.
[Enemy feedback](../crates/app/src/enemy_feedback.rs) owns timing and history;
`state.horde_preview.flashing_instances` reports prepared flashing instances.

## Disintegrate prototype

Hold **Space** or **3**, aim with the cursor, and release to stop. Channeling pauses
powered walking but permits physical displacement. An active melee swing must
finish first; selecting melee ends the channel on the next tick.

The rendered beam starts at the sampled `BeamOrigin` sword-tip joint. Damage
uses the player-centred capsule, independently of animation. It pierces enemies
until a wall, pulses damage, and applies a forward impulse on each touching tick.
Release resets cadence. [Beam](../crates/sim/src/beam.rs) owns reach, width and timing.
There is no resource cost or damage ramp.

`Game::beam` and `state.sim.beam` expose endpoints, tuning, channel state, pulse
clock and completed-tick `hit_positions`, including lethal hits. Melee-only
`attack_discs` and collision outlines do not describe the beam capsule.
Scenarios hold channels with `channels: [(at: 0, ticks: 24)]` (half-open interval).

Presentation uses the [wall-clipped](walls.md#attacks) completed-tick pose. Its
sword-to-beam path is checked against walls separately, so animation cannot draw
through an obstruction missed by the damage ray. Release removes the beam
immediately; pose easing and short-lived hit sparks are visual only.
`state.character_preview.channel_weight` reports the actual overlay during
entry and release. Visual durations and geometry stay with their app owners.
