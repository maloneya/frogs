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

## Enemy swipe

Each marked melee attacker brakes within striking distance, leans back
for a 0.6-second windup, then snaps forward for a short directional swipe.
An amber disc fills toward impact; red marks the active damage window.
The direction commits at windup, so sidestepping can avoid it. Each swipe can
remove only one health, followed by 0.65 seconds of recovery before pursuit
resumes. Physical knockback still moves the attacker and walls block damage.
The marker uses the strike radius; the player's collision radius also counts
for overlap. The animation reuses the bounded crowd poses with an instance tilt.

[Action definitions and execution](behaviors.md) own timing and geometry.
`actors` reports phase, elapsed/duration ticks, committed direction and hit state;
restart and scene eviction clear commitments.

## Enemy leap

Marked attackers also leap from 3–7 metres away. A purple landing disc and
backward lean warn for 0.7 seconds; the target point locks immediately. The
0.6-second jump follows that point, not the moving player, and can deal one
health only on landing. The disc flashes red on impact, followed by 0.9 seconds
of recovery. Each enemy waits at least four seconds between leap commitments
and can still swipe up close.

At most four attacks can be committed together, including recovery, with new
commitments spaced by 0.2 seconds and admitted in rotating order. Both attacks
can be granted to explicit bodies, formations or source emissions. `actors`
reports identity, action, phase, landing target and action cooldowns.

The jump adds real ground travel through the shared motor, plus a visual arc
1.8 metres high. Ground-plane walls and body contacts still apply throughout
the jump, and airborne enemies remain damageable; it does not grant collision
immunity or allow jumping over walls. Landing damage requires actual physical
reach as well as overlap with the promised disc, so blocked jumps cannot hit
remotely. `scenes/leaping_horde.ron` is a small playable mixed-attack trial.
