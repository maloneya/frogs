# Corpses

The lethal tick stops actor actions, counts the defeat, and admits a corpse.
The same record follows shared physics while momentum and contacts carry the
body, then keeps its final position after physical removal. Death age never
waits for rest or restarts on another impact.

Presentation starts the authored `Death` clip immediately, blending from the
last visible pose and preserving heading and airborne height. Four bounded
shared collapse palettes advance the clip; its final pose stays frozen until
expiration. No locomotion clip is selected for a dead body. The collider remains
the existing ground-plane proxy; this is authored animation, not a ragdoll.

[The corpse owner](../crates/game/src/corpse.rs) defines lifetime and capacity.
The ten-second lifetime starts at death and also bounds bodies kept moving by
continued forces. At capacity, later deaths leave no remains and their physical
bodies retire immediately. Expiration frees space before new admission.
Administrative despawn creates no corpse and removes an existing moving corpse's
record. Scene eviction removes its remains, and restart clears all remains.
