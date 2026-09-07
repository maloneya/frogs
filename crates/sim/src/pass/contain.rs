//! Keeps every body inside the arena.

use glam::Vec2;

use super::motion::Physics;
use crate::trace::{Event, TraceSink};
use crate::{ARENA_HALF, ENEMY_RADIUS, EntityId, PLAYER_RADIUS};

/// What a clamp does to a NaN, and why this pass has to refuse one.
///
/// `f32::max` returns the operand that is *not* NaN, so clamping a poisoned
/// position does not propagate the poison — it silently replaces it with the
/// arena limit. Every NaN body teleports to the same corner, and by the end of
/// the tick every position is finite again and looks legal.
///
/// That laundering is worse than the NaN. The real fault is upstream, in the
/// solver, where two coincident bodies were normalised without a guard; the
/// symptom is "the entire horde jumped to the corner", which points at this
/// pass and at the arena bounds instead. So this is asserted where the poison
/// arrives rather than left to be discovered where it lands.
const POISONED: &str = "a position was NaN or infinite before it was clamped. \
                        The clamp would have hidden it by returning the arena limit. \
                        Look upstream in the solver for a normalise of a zero-length \
                        difference — two bodies at exactly the same point";

/// Clamps positions to the floor, and reports how many bodies it had to stop.
///
/// The limit is inset by each body's radius, so it is the *body* that stops at
/// the wall rather than its centre. A version that clamped to `ARENA_HALF`
/// would leave every cornered body standing halfway inside the floor's edge.
///
/// Runs after separation rather than before: clamping first would let a contact
/// shove a body straight through the wall and leave it outside until something
/// else happened to touch it.
pub(crate) fn contain(
    pos: &mut [Vec2],
    ids: &[EntityId],
    physics: &mut Physics,
    mut trace: TraceSink<'_>,
) {
    let (player, horde) = pos.split_first_mut().expect("the player always exists");
    let mut stopped = 0;

    debug_assert!(player.is_finite(), "{}", POISONED);

    let player_limit = Vec2::splat(ARENA_HALF - PLAYER_RADIUS);
    let clamped = player.clamp(-player_limit, player_limit);
    if clamped != *player {
        stopped += 1;
        stop_at_wall(physics, ids[0], *player, clamped);
        *player = clamped;
    }

    let enemy_limit = Vec2::splat(ARENA_HALF - ENEMY_RADIUS);
    for (i, pos) in horde.iter_mut().enumerate() {
        debug_assert!(pos.is_finite(), "{}", POISONED);

        let clamped = pos.clamp(-enemy_limit, enemy_limit);
        if clamped != *pos {
            stopped += 1;
            stop_at_wall(physics, ids[i + 1], *pos, clamped);
            *pos = clamped;
        }
    }

    // Only when something happened. An event every tick saying "nothing was
    // clamped" would fill the ring buffer with the absence of news and push out
    // the rare events it exists to keep.
    if stopped > 0 {
        trace.emit(Event::Clamped { count: stopped });
    }
}

/// Each axis is independent, including a corner: cancel both outward components.
fn stop_at_wall(physics: &mut Physics, id: EntityId, pos: Vec2, clamped: Vec2) {
    if pos.x != clamped.x {
        physics.wall(id, Vec2::new(pos.x.signum(), 0.0));
    }
    if pos.y != clamped.y {
        physics.wall(id, Vec2::new(0.0, pos.y.signum()));
    }
}
