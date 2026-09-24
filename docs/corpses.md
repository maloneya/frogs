# Corpses

Combat defeats leave nonphysical remains at the enemy's final ground position.
[Game's corpse owner](../crates/game/src/corpse.rs) defines the count budget and
lifetime. At capacity, later deaths leave no corpse; expiring remains free space
before the same tick admits new deaths. Admission follows deterministic engine
defeat order. Corpses never enter body, collision, health or encounter membership.

The game ages remains once per fixed tick after physics and consumes the current
lossless defeat outcomes, independent of diagnostic trace retention. Evicting the
owning scene clears its remains; restart and successful scene replacement clear
all remains. Administrative despawn creates none. Failed scene loads preserve them.
Positions, ownership and remaining lifetime participate in replay hashes and reports.

Presentation draws the current enemy mesh lying on its back using one frozen idle
pose shared by all corpses. This adds one bounded pose bucket to the existing living
horde buckets. It is a static visual, with no falling animation or ragdoll motion.
The visible lifetime and count cap are initial tuning choices to judge in combat.

The [lifetime scenario](../scenarios/corpses_expire_after_ten_seconds.ron) checks
both sides of expiry and walking through remains. Game lifecycle tests cover full
admission, capacity reopening, scene cleanup, restart and refused loads.
