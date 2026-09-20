# Attack effects

Cleave leaves a blue crescent; Slam produces an amber expanding ring with a
faint interior wash. Both use the committed collision samples exposed by
`Game::attack_discs`, filtered to the active phase. Changing the selected attack
mid-swing cannot change its effect.

The app observes every completed game tick, even when several ticks occur before
a frame. A bounded history stores world-space positions and radii. Cleave joins
those observations into a tapered ribbon at presentation height; old samples do
not rotate with the player. Slam interpolates the latest two centres and radii,
so it follows the live hitbox while active. During recovery its final ring stays
where it ended and fades. The fade lasts eight ticks (about 133 ms); it never
extends the damage window. Scene start/restart clears the entire history.

Rendering uses the same interpolation fraction as body presentation, revealing
the newest segment between the previous and current observations. The ribbon
interpolates angles around each observed player origin, rather than cutting a
straight chord across the arc. These visual interpolants never become collision
geometry. The effect communicates attack reach, not the exact animated sword
trajectory.

The renderer accepts only generic world-space coloured triangles, drawn in one
batched call after opaque geometry and before diagnostics/HUD. Alpha blending
uses linear colour; depth testing hides effects behind opaque objects, and
effects never write depth. Vertex construction rejects non-finite data, and the
GPU upload checks both capacity and complete triangles. Presentation owns the
colours, height, taper, fade, and choice of ribbon versus ring.

`state.attack_effects` reports the observed tick, active phase flag, retained
sample count, latest committed profile/centre/radius, and generated vertex count.
`state.render.effect_vertices` and `effect_draws` report workload (zero or one
draw). Observation, history, and geometry are presentation-only and do not enter
the game hash or diagnostic trace.

Tests cover both attack windows, mid-swing profile changes, skipped render
frames, duplicate observations, history expiry, turning, arc interpolation,
unchanged game hashes, and the app's restart boundary. A headless GPU pixel test
checks alpha blending, depth occlusion, and clearing the stream. The existing
attack scenarios continue to verify the authoritative collision behaviour.

## Disintegrate prototype

Hold **3** for Disintegrate and aim with the cursor. Release to stop. The channel pauses powered walking, while shared physics
can still displace the player. It follows current aim every simulation tick.
The existing Slam animation is held in a forward pose for this prototype.

`sim::Beam` owns reach, radius and pulse cadence: 12 metres, 0.3 metre half-width,
one hit immediately and every 12 ticks thereafter. With three-hit enemies this
kills a stationary target on ticks 0, 12 and 24 of an uninterrupted channel.
The capsule tests final body positions before defeat removal, includes enemy
body radius, pierces all enemies, and applies no impulse. Scenery does not block
it yet; there is no resource cost or damage ramp. Releasing resets the cadence.
A committed melee swing must finish before channeling can begin; selecting a
melee profile ends the channel on the next tick.

`Game::beam` exposes the authoritative capsule; `state.sim.beam` reports its
endpoints and tuning. `channeling` and `channel_tick` report active state and
pulse clock. Traces emit channel start/stop and individual damage events.
`attack_discs` continues to describe melee only, so F3's disc overlay does not
outline this capsule. Restart clears channel state; scene eviction needs no
beam target cleanup because pulses retain no target identities.

Presentation draws three orange/gold bands using the existing linear-colour
triangle stream, interpolating origin and angle between completed ticks. History
is bounded and release removes the beam immediately. It never feeds geometry
back into damage. Scenarios use `channels: [(at: 0, ticks: 24)]` for a half-open
hold; `attacks` still supplies discrete swing press edges.
