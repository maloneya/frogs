//! Keeps every body inside the arena.

use glam::Vec2;

use crate::trace::{Event, TraceSink};
use crate::{ARENA_HALF, ENEMY_RADIUS, PLAYER_RADIUS};

/// Clamps positions to the floor, and reports how many bodies it had to stop.
///
/// The limit is inset by each body's radius, so it is the *body* that stops at
/// the wall rather than its centre. A version that clamped to `ARENA_HALF`
/// would leave every cornered body standing halfway inside the floor's edge.
///
/// Runs after separation rather than before: clamping first would let a contact
/// shove a body straight through the wall and leave it outside until something
/// else happened to touch it.
pub(crate) fn contain(player: &mut Vec2, horde: &mut [Vec2], mut trace: TraceSink<'_>) {
    let mut stopped = 0;

    let player_limit = Vec2::splat(ARENA_HALF - PLAYER_RADIUS);
    let clamped = player.clamp(-player_limit, player_limit);
    if clamped != *player {
        stopped += 1;
        *player = clamped;
    }

    let enemy_limit = Vec2::splat(ARENA_HALF - ENEMY_RADIUS);
    for pos in horde.iter_mut() {
        let clamped = pos.clamp(-enemy_limit, enemy_limit);
        if clamped != *pos {
            stopped += 1;
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
