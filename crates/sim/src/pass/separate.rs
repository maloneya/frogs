//! Immediate contact response: grid-based crowd pairs, then a linear player pass.
//! Pair order affects the result of the single sweep.

use glam::Vec2;

use super::motion::Physics;
#[cfg(test)]
use super::motion::{ENEMY_INV_MASS, PLAYER_INV_MASS};
use crate::contact::{self, Contact};
use crate::trace::{Event, TraceSink};
use crate::{ENEMY_RADIUS, EntityId, PLAYER_ENEMY_CONTACT_DISTANCE};

/// Resolves player contacts after crowd contacts, prioritizing player space
/// when a single sweep leaves residual overlaps.
pub(crate) fn player(
    pos: &mut [Vec2],
    ids: &[EntityId],
    physics: &mut Physics,
    mut trace: TraceSink<'_>,
) -> usize {
    let (player, horde) = pos.split_first_mut().expect("the player always exists");
    let contact_distance = PLAYER_ENEMY_CONTACT_DISTANCE;
    let mut contacts = 0;
    let mut transfers = 0;

    for (i, enemy) in horde.iter_mut().enumerate() {
        // Ask, then answer. The index is the coincident-pair tiebreak, which is
        // what keeps a crowd stacked on one point fanning out reproducibly.
        if let Some(found) = contact::between(*player, *enemy, contact_distance, i) {
            resolve(
                player,
                enemy,
                found,
                physics.get(ids[0]).expect("physical player").inverse_mass(),
                physics.get(ids[i + 1]).expect("physical body").inverse_mass(),
            );
            transfers += usize::from(physics.collide(ids[0], ids[i + 1], found));
            contacts += 1;
        }
    }

    // Summarised, not per pair: a thousand overlapping bodies would otherwise
    // emit a thousand events a tick and flush the ring buffer of everything
    // rare enough to be worth keeping.
    if contacts > 0 {
        trace.emit(Event::Contacts { count: contacts });
    }

    if transfers > 0 {
        trace.emit(Event::Momentum { pairs: transfers });
    }
    contacts
}

/// Resolves nearby pairs once in ascending dense-row order with immediate response.
/// Grid updates preserve contacts created by earlier corrections. Dense piles can
/// still require quadratic work. One sweep leaves residual overlap to settle on
/// later ticks rather than making the crowd a rigid constraint.
pub(crate) fn crowd(
    horde: &mut [Vec2],
    ids: &[EntityId],
    physics: &mut Physics,
    grid: &mut crate::grid::Grid,
    mut trace: TraceSink<'_>,
) -> crate::CrowdWork {
    let contact_distance = 2.0 * ENEMY_RADIUS;
    let mut contacts = 0;
    let mut transfers = 0;
    let mut visit = |i: usize, j: usize, a: &mut Vec2, b: &mut Vec2| {
        if let Some(found) = contact::between(*a, *b, contact_distance, i + j) {
            let (aid, bid) = (ids[i], ids[j]);
            resolve(
                a, b, found,
                physics.get(aid).expect("physical body").inverse_mass(),
                physics.get(bid).expect("physical body").inverse_mass(),
            );
            transfers += usize::from(physics.collide(aid, bid, found));
            contacts += 1;
        }
    };
    #[cfg(test)]
    let candidates = if grid.brute_force { brute_force(horde, &mut visit) }
        else { grid.solve(horde, &mut visit) };
    #[cfg(not(test))]
    let candidates = grid.solve(horde, &mut visit);

    if contacts > 0 { trace.emit(Event::Crowded { count: contacts }); }
    if transfers > 0 { trace.emit(Event::Momentum { pairs: transfers }); }
    let possible_pairs = horde.len() as u64 * horde.len().saturating_sub(1) as u64 / 2;
    crate::CrowdWork { candidates, possible_pairs, contacts }
}

/// Executable reference retained only in engine tests; shares contact response
/// but independently visits every pair, including ones a stale grid would miss.
#[cfg(test)]
fn brute_force(horde: &mut [Vec2], mut visit: impl FnMut(usize, usize, &mut Vec2, &mut Vec2)) -> u64 {
    for i in 0..horde.len() {
        let (head, tail) = horde.split_at_mut(i + 1);
        for (offset, b) in tail.iter_mut().enumerate() { visit(i, i + 1 + offset, &mut head[i], b); }
    }
    horde.len() as u64 * horde.len().saturating_sub(1) as u64 / 2
}

/// Pushes two overlapping bodies apart along the contact normal, splitting the
/// correction between them by inverse mass.
///
/// Projection repairs geometry without modifying velocity. `Physics::collide`
/// separately exchanges momentum along the same contact's normal. Keeping the
/// two responses separate prevents an initially overlapping pile from acquiring
/// energy just because its positions needed correction.
fn resolve(a: &mut Vec2, b: &mut Vec2, contact: Contact, a_inv_mass: f32, b_inv_mass: f32) {
    // Two immovable bodies have no correction to share out. Bailing keeps the
    // division below from being a zero-divide that quietly yields NaN.
    let share = a_inv_mass + b_inv_mass;
    if share <= 0.0 {
        return;
    }

    *a -= contact.normal() * (contact.depth() * a_inv_mass / share);
    *b += contact.normal() * (contact.depth() * b_inv_mass / share);
}

/// Exchange momentum before integration when fast bodies would pass each other.
/// Reads current positions and predicted physical travel, writes only velocity.
/// One ordered sweep, like ordinary contacts; collisions may occur up to one
/// tick early. Slow ticks skip the pair scan entirely. Walls shield pairs.
pub(crate) fn prevent_crossings(
    pos: &[Vec2],
    ids: &[EntityId],
    physics: &mut Physics,
    walls: &crate::wall::Walls,
    dt: crate::Dt,
    mut trace: TraceSink<'_>,
) {
    let fast = |velocity: Vec2| velocity.length_squared() * dt.secs().powi(2)
        > ENEMY_RADIUS * ENEMY_RADIUS;
    if !ids.iter().any(|&id| fast(physics.get(id).expect("physical body").velocity())) {
        return;
    }
    let mut transfers = 0;
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let a = physics.get(ids[i]).expect("physical body").velocity();
            let b = physics.get(ids[j]).expect("physical body").velocity();
            if !fast(a) && !fast(b) { continue; }
            let radius = if i == 0 { PLAYER_ENEMY_CONTACT_DISTANCE } else { 2.0 * ENEMY_RADIUS };
            if let Some(contact) = contact::crossing(pos[j] - pos[i], (b - a) * dt.secs(), radius)
                && !walls.obstructs(pos[i], pos[j])
            {
                transfers += usize::from(physics.collide(ids[i], ids[j], contact));
            }
        }
    }
    if transfers > 0 { trace.emit(Event::Momentum { pairs: transfers }); }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the pair the way `separate` does — ask, then answer — so these
    /// stay tests of the *response* rather than re-testing the query. Returns
    /// whether there was anything to respond to.
    fn settle(
        a: &mut Vec2,
        b: &mut Vec2,
        distance: f32,
        ia: f32,
        ib: f32,
        tiebreak: usize,
    ) -> bool {
        let Some(found) = contact::between(*a, *b, distance, tiebreak) else { return false };
        resolve(a, b, found, ia, ib);
        true
    }

    /// What separation is *for*: afterwards the two are exactly touching, not
    /// merely less overlapped.
    #[test]
    fn separating_leaves_two_bodies_exactly_touching() {
        let mut a = Vec2::new(-0.1, 0.0);
        let mut b = Vec2::new(0.1, 0.0);

        assert!(settle(&mut a, &mut b, 1.0, 1.0, 1.0, 0));
        assert!((a.distance(b) - 1.0).abs() < 1e-5, "settled at {}", a.distance(b));
    }

    /// Bodies that are merely near must not be touched at all, or the solver
    /// would jitter everything within reach of everything.
    #[test]
    fn bodies_that_do_not_overlap_are_left_alone() {
        let mut a = Vec2::new(-1.0, 0.0);
        let mut b = Vec2::new(1.0, 0.0);

        assert!(!settle(&mut a, &mut b, 1.0, 1.0, 1.0, 0));
        assert_eq!((a, b), (Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0)));
    }

    /// Coincident bodies must come apart to a real distance rather than to a
    /// pair of NaNs. The finite *normal* is the query's job and is tested
    /// there; this is the half that says the response actually uses it.
    #[test]
    fn coincident_bodies_separate_instead_of_producing_nan() {
        let mut a = Vec2::new(5.0, -3.0);
        let mut b = Vec2::new(5.0, -3.0);

        assert!(settle(&mut a, &mut b, 1.0, 1.0, 1.0, 7));

        assert!(a.is_finite() && b.is_finite(), "poisoned: {a} {b}");
        assert!((a.distance(b) - 1.0).abs() < 1e-5);
    }

    /// Projection moves bodies apart without moving the pair: the mass-weighted
    /// centre is unchanged. That is what distinguishes it from an impulse — it
    /// can only redistribute position, never inject energy, so there is no
    /// explosive pushback to suppress.
    #[test]
    fn separation_preserves_the_mass_weighted_centre() {
        let (ia, ib) = (PLAYER_INV_MASS, ENEMY_INV_MASS);
        let centre = |a: Vec2, b: Vec2| (a / ia + b / ib) / (1.0 / ia + 1.0 / ib);

        let mut a = Vec2::new(0.2, -0.1);
        let mut b = Vec2::new(-0.1, 0.2);
        let before = centre(a, b);

        settle(&mut a, &mut b, 1.0, ia, ib, 0);
        assert!((centre(a, b) - before).length() < 1e-5);
    }

    /// Two immovable bodies cannot be pushed apart, and asking must not divide
    /// by their combined zero and yield NaN. Corpses and props will be exactly
    /// this case.
    ///
    /// Note this now *does* report a contact — they are overlapping, which is a
    /// true fact — and then declines to act on it. Splitting the query from the
    /// response is what makes those two different answers, and a trigger volume
    /// on an immovable prop is the case that needs them to be.
    #[test]
    fn two_immovable_bodies_are_left_where_they_are() {
        let mut a = Vec2::ZERO;
        let mut b = Vec2::new(0.1, 0.0);

        assert!(settle(&mut a, &mut b, 1.0, 0.0, 0.0, 0), "they do overlap");
        assert_eq!((a, b), (Vec2::ZERO, Vec2::new(0.1, 0.0)), "but neither may move");
    }

    /// **The mechanic, at the level of one contact.** The heavy body barely
    /// moves; the light one does almost all the yielding. This is the whole of
    /// why a single enemy is a nudge rather than a wall.
    #[test]
    fn the_heavier_body_yields_less() {
        let mut player = Vec2::ZERO;
        let mut enemy = Vec2::new(0.5, 0.0);
        let (start_player, start_enemy) = (player, enemy);

        settle(&mut player, &mut enemy, 1.0, PLAYER_INV_MASS, ENEMY_INV_MASS, 0);

        let player_moved = player.distance(start_player);
        let enemy_moved = enemy.distance(start_enemy);
        assert!(
            enemy_moved > player_moved * 10.0,
            "expected a lopsided split, got {player_moved} vs {enemy_moved}"
        );
    }
}
