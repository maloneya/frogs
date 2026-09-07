//! Pushes overlapping bodies apart.
//!
//! **The response half of a contact.** Whether two bodies touch, and along what
//! line, is [`crate::contact`]'s question; this module only decides what to do
//! about the answer. An attack asks the same question and does something else
//! entirely with it.
//!
//! ## Two passes, one module
//!
//! [`crowd`] resolves the horde against itself; [`player`] resolves the horde
//! against the player. They are separate entries in the schedule because the
//! **order between them is load-bearing** and belongs somewhere readable, and
//! they share this module because they are the same behaviour over different
//! pair sets — same response, same constants, and only the pairing differs.
//!
//! They also diverge in cost in a way that matters later: [`player`] is O(N)
//! and [`crowd`] is O(N^2), so the uniform grid, when it lands, optimises one
//! of them and leaves the other alone.

use glam::Vec2;

use super::motion::Physics;
#[cfg(test)]
use super::motion::{ENEMY_INV_MASS, PLAYER_INV_MASS};
use crate::contact::{self, Contact};
use crate::trace::{Event, TraceSink};
use crate::{ENEMY_RADIUS, EntityId, PLAYER_RADIUS};

/// Separates every body overlapping the player, and reports how many.
///
/// Brute force, deliberately. It is O(N), so at the default horde it is a
/// thousand distance checks a tick and costs nothing worth measuring.
///
/// **Runs after [`crowd`]**, and that order is a decision rather than an
/// accident. One Gauss-Seidel sweep leaves residual overlap wherever a body was
/// pushed by two things at once, and whichever of these two runs *last* is the
/// one whose result survives the tick. Last here means the player's personal
/// space is inviolate: no enemy ends a tick standing inside it. The residual
/// lands instead on enemy-against-enemy, where it is a fraction of a body's
/// width inside a mass of identical cubes and nobody can see it. Reverse the
/// order and the visible artefact is a body clipping into the character the
/// camera is centred on.
pub(crate) fn player(
    pos: &mut [Vec2],
    ids: &[EntityId],
    physics: &mut Physics,
    mut trace: TraceSink<'_>,
) -> usize {
    let (player, horde) = pos.split_first_mut().expect("the player always exists");
    let contact_distance = PLAYER_RADIUS + ENEMY_RADIUS;
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

/// Separates the horde against itself, and reports how many pairs it resolved.
///
/// **This is what makes the horde a crowd** rather than a grid of positions
/// drawn near one another. Without it, shoving into the pack compresses it to a
/// point instead of displacing it outward.
///
/// Every unordered pair once — `j` starts past `i` — so a pair is never
/// resolved twice, which would double the correction and make the crowd
/// springy.
///
/// **O(N^2), and knowingly so.** Measured in release, per tick, against a
/// 16.7ms frame:
///
/// | bodies | pairs | cost | share of a frame |
/// |---|---|---|---|
/// | 1024 | 523k | 0.14ms | 0.8% |
/// | 4096 | 8.4M | 2.1ms | 12% |
/// | 16384 | 134M | 32.6ms | 195% |
///
/// So the grid is not urgent at the size the game runs, and these numbers are
/// what say so rather than a feeling. They also say when it stops being
/// optional: somewhere past 4096 this pass alone is the frame.
///
/// Micro-optimising it is not the answer either — from about 4096 the position
/// array leaves L1 and the loop is memory-bound, so the arithmetic is no longer
/// what costs. Only a broadphase changes the shape.
///
/// When it comes, it is an *optimisation of something already correct*, and the
/// way it gets tested is by producing the same contact set as this over a
/// replayed input stream — a test that can only exist if this exists first.
///
/// Gauss-Seidel, as [`player`] is: each correction is written immediately, so
/// the next pair sees it. That converges faster per sweep than accumulating and
/// applying at the end, and its one real cost — the result depends on the order
/// pairs are visited — is fine because the order is a fixed walk over storage
/// rather than anything that varies run to run. It is also why
/// [`crate::pass::remember`] copies rather than swapping two buffers.
///
/// **One sweep, not iterated to convergence.** A body squeezed between two
/// others ends the tick still slightly overlapped, and the next tick takes
/// another bite. That reads as a crowd settling, which is what it should look
/// like; iterating here to a hard constraint would instead make a dense pack
/// rigid and jolt everything the moment the player entered it.
pub(crate) fn crowd(
    horde: &mut [Vec2],
    ids: &[EntityId],
    physics: &mut Physics,
    mut trace: TraceSink<'_>,
) -> usize {
    let contact_distance = 2.0 * ENEMY_RADIUS;
    let mut contacts = 0;
    let mut transfers = 0;

    for i in 0..horde.len() {
        // `split_at_mut` is what makes two simultaneous `&mut` into one slice
        // legal. The alternative — index and copy out, compute, write back — is
        // the same arithmetic with the borrow checker switched off for the one
        // thing it is actually good at here.
        let (head, tail) = horde.split_at_mut(i + 1);
        let a = &mut head[i];

        for (offset, b) in tail.iter_mut().enumerate() {
            // Every pair gets its own tiebreak, so a stack of coincident bodies
            // fans out instead of every pair escaping along the same line and
            // re-stacking next tick.
            let tiebreak = i + (i + 1 + offset);

            if let Some(found) = contact::between(*a, *b, contact_distance, tiebreak) {
                let (aid, bid) = (ids[i], ids[i + 1 + offset]);
                resolve(
                    a,
                    b,
                    found,
                    physics.get(aid).expect("physical body").inverse_mass(),
                    physics.get(bid).expect("physical body").inverse_mass(),
                );
                transfers += usize::from(physics.collide(aid, bid, found));
                contacts += 1;
            }
        }
    }

    if contacts > 0 {
        trace.emit(Event::Crowded { count: contacts });
    }

    if transfers > 0 {
        trace.emit(Event::Momentum { pairs: transfers });
    }
    contacts
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
