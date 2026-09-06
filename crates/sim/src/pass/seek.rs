//! Walks the bodies that chase toward the thing they are chasing.
//!
//! **The first behaviour that only some bodies have**, and the shape every one
//! after it should copy. It is three things and no more: a [`Members`] set
//! saying who has it, a pass that walks that set, and the constants the pass
//! tunes itself with. Adding it required a field on `World` and a line in the
//! schedule; it changed no existing type, and a body that does not seek pays
//! nothing for the fact that seeking exists.
//!
//! That last part is what the sparse set buys. The pass iterates *membership*,
//! not the horde, so three chasers among a thousand bodies cost three
//! iterations. The wide-table alternative would cost a thousand iterations and
//! a thousand branches to find the same three.
//!
//! A behaviour that needs per-member data keeps its own arrays beside the set
//! and pushes to them at the row [`Members::add`] hands back. This one needs
//! none: every chaser runs at the same speed, and inventing a per-body speed
//! nothing varies would be storage built for an imagined requirement.

use glam::Vec2;

use crate::members::Members;
use crate::slots::Slots;
use crate::{Dt, ENEMY_RADIUS, PLAYER_RADIUS};

/// How fast a chaser moves, in world units per second.
///
/// Slower than the player, and that is the single most important thing about
/// this number rather than a detail of it. A horde you cannot outrun is not a
/// horde, it is a countdown: kiting, spacing and the decision of when to stand
/// and fight all stop existing at the moment this reaches
/// [`crate::pass::walk::PLAYER_SPEED`].
const SPEED: f32 = 3.5;
const _: () = assert!(SPEED > 0.0);
const _: () = assert!(
    SPEED < crate::pass::walk::PLAYER_SPEED,
    "a horde that cannot be outrun deletes kiting, which is most of ARPG movement"
);

/// How close a chaser gets before it stops walking.
///
/// Exactly the distance at which it is touching, so a chaser arrives against
/// the player and stands there rather than grinding into them. Without it the
/// two passes fight every tick — seek walks the body in, separation pushes it
/// out, forever — which costs a contact per chaser per tick and fills the trace
/// with a crowd that is not going anywhere.
///
/// Derived from the radii rather than written down, so it cannot disagree with
/// the contact distance `pass::separate` uses for the same pair.
const STOP_AT: f32 = PLAYER_RADIUS + ENEMY_RADIUS;

/// Walks every chaser toward `target`.
///
/// **Takes the membership and the bodies separately**, which is the pass
/// contract doing real work: it declares that it reads who chases, reads where
/// everything is, and writes only positions. It cannot grant the behaviour to
/// anything, and it cannot spawn or kill.
///
/// Ids that no longer resolve are skipped rather than treated as a bug. A
/// behaviour set holding a name whose body has died is the ordinary case — it
/// is what a generational id makes safe to ask about — and `World::despawn_enemy`
/// revokes eagerly anyway, so this is the belt to that pair of braces.
pub(crate) fn seek(seekers: &Members, bodies: &Slots, pos: &mut [Vec2], target: Vec2, dt: Dt) {
    let full_step = SPEED * dt.secs();

    for &id in seekers.ids() {
        let Some(row) = bodies.index(id) else { continue };
        let body = &mut pos[row];

        let to_target = target - *body;
        let distance = to_target.length();

        // Already arrived, or close enough that the step would carry it past.
        // Clamping to what is left rather than overshooting and letting
        // separation clean it up: the same discipline `pass::face` uses when it
        // clamps a turn to the remaining arc, and for the same reason — an
        // overshoot corrected elsewhere is a jitter whose cause is two passes
        // away from where it shows up.
        let remaining = distance - STOP_AT;
        if remaining <= 0.0 {
            continue;
        }

        // `distance` is strictly greater than `STOP_AT` here, which is
        // positive, so it cannot be zero and this cannot be a divide by it.
        *body += (to_target / distance) * full_step.min(remaining);
    }
}
