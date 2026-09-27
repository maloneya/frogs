# Corpses

Combat defeats leave nonphysical remains at the enemy's final position.
[The corpse owner](../crates/game/src/corpse.rs) defines lifetime and capacity.
At capacity, later deaths leave none; expiration frees space before new admission.
Administrative despawn creates no corpse. Scene eviction removes its remains,
and restart clears all remains.

Presentation lays the enemy mesh on its back with a shared frozen idle pose.
There is no falling animation, ragdoll motion or collision.
