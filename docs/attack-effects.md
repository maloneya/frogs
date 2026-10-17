# Attack effects

Cleave leaves a pale blue crescent from committed active hitbox samples. Its
lateral offset is reflected about the committed facing direction to follow the
right-handed sword's sweep. Damage discs retain their gameplay order; the visual
history keeps each observed world origin, reach and radius while fading.

Slam plants the staff butt on the first active sample. A cyan-white contact flash
releases a raised, translucent pressure wave and outward magical wisps. The wave
expands to the committed attack's final radius over its active samples, then
dissolves; the wisps fade over 24 ticks. The origin comes from the equipped
weapon's sampled butt, derived from its rigid mesh at admission. It locks in
world space at contact, including when ticks pass without a redraw. The same
release appears on empty ground. This visual wave is offset from the
player-centred damage discs; it is not an exact collision outline.
Changing the selected attack mid-swing does not change the committed effect.
Fading never extends the damage window. Start/restart clears history.

[App effect code](../crates/app/src/attack_effects.rs) owns geometry, interpolation and visual tuning.
The renderer draws blended triangles after opaque geometry, with depth testing
but no depth writes. Visual interpolation does not become collision geometry.

`state.attack_effects` reports observed tick, active state, retained Cleave/beam
samples, their profile/centre/radius and vertex count. `slam_impact`, `slam_centre` and `slam_tick` identify the
separately retained ground burst. `state.render.effect_vertices`
and `effect_draws` report rendering workload.

## Contact flash

Surviving enemies flash white when their observed health drops. Feedback begins
when damage can first be rendered; catch-up ticks collapse to the latest health.
Repeated redraws do not refresh it. Damage observations also select three shared flinch stages for 0.3 seconds,
without interrupting gameplay movement or attacks. Recoil faces against observed
displacement, falling back to the player direction for stationary targets; this
is presentation inference. Melee contact bursts include survivors and lethal
outcomes. Defeated characters play their authored collapse immediately, while
physical momentum can still carry them. The [corpse owner](corpses.md) describes
the shared collapse poses, final pose and death-clock lifetime.
[Enemy feedback](../crates/app/src/enemy_feedback.rs) owns timing and history;
`state.horde_preview.flashing_instances` reports prepared flashing instances.

## Disintegrate prototype

Hold **Space** or **3**, aim with the cursor, and release to stop. Channeling pauses
powered walking but permits physical displacement. An active melee swing must
finish first; selecting melee ends the channel on the next tick.

The rendered beam starts at the sampled `BeamOrigin` sword-tip joint. Damage
uses the player-centred capsule, independently of animation. It pierces enemies
until a wall or physical panel, pulses eligible damage, and applies a forward
impulse to each unshielded contacted body on every touching tick, independently
of its health or allegiance.
Release resets cadence. [Disintegrate](../crates/game/src/player.rs) owns reach, width and timing;
[simulation geometry](../crates/sim/src/beam.rs) owns capsule contact and clipping.
There is no resource cost or damage ramp.

`Game::beam` and `state.sim.beam` expose endpoints, tuning, channel state, pulse
clock and completed-tick `hit_positions`, including lethal hits. Melee-only
`attack_discs` and collision outlines do not describe the beam capsule.
Scenarios hold channels with `channels: [(at: 0, ticks: 24)]` (half-open interval).

Presentation uses the [solid-clipped](walls.md#attacks) completed-tick pose. Its
sword-to-beam path uses `Game::solid_hit` against walls and current jointed boxes, sharing
the beam's nearest-hit geometry so animation cannot draw through an obstruction
missed by the damage ray. Release removes the beam
immediately; pose easing and short-lived hit sparks are visual only.
`state.character_preview.channel_weight` reports the actual overlay during
entry and release. Visual durations and geometry stay with their app owners.

## Enemy swipe

An actor with a Swipe action brakes within striking distance, leans back, then
snaps forward for a short directional strike. With the default action settings,
the windup lasts 0.6 seconds.
An amber disc fills toward impact; red marks the active damage window.
The direction commits at windup, so sidestepping can avoid it. Each swipe can
remove only one health, followed by 0.65 seconds of recovery before pursuit
resumes. Each contacted body also receives its authored physical impulse,
including neutral and healthless objects. Physical knockback still moves the
attacker, and walls or jointed bodies can shield targets.
The marker uses the strike radius; the player's collision radius also counts
for overlap. The wolf plays its authored `Bite` clip through the committed phases,
using bounded shared attack poses.

[Action definitions and execution](behaviors.md) own timing and geometry.
`actors` reports phase, elapsed/duration ticks, committed direction and hit state;
restart and scene eviction clear commitments.

## Enemy leap

An actor with a Leap action uses its authored range and timing. At the defaults,
it leaps from 3–7 metres away. A purple landing disc and
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
remotely. Use the [scene editor](scene-editor.md) to compose a mixed-attack trial.
