//! Pushes overlapping bodies apart.

use glam::Vec2;

use crate::trace::{Event, TraceSink};
use crate::{ENEMY_RADIUS, PLAYER_RADIUS};

/// How hard each body resists being pushed.
///
/// The player is twenty times heavier than a single enemy, which is what lets
/// it wade into a crowd and displace it rather than be carried off by it.
const PLAYER_INV_MASS: f32 = 0.05;
const ENEMY_INV_MASS: f32 = 1.0;
const _: () = assert!(PLAYER_INV_MASS > 0.0 && ENEMY_INV_MASS > 0.0);
const _: () = assert!(
    PLAYER_INV_MASS < ENEMY_INV_MASS,
    "an equal or lighter player is shoved around by fodder"
);

/// Below this separation two bodies have no line between them to push along,
/// and normalising their difference yields NaN — a position no clamp recovers.
/// Squared, since that is what the overlap test already has to hand.
const COINCIDENT_SQ: f32 = 1e-12;
const _: () = assert!(COINCIDENT_SQ > 0.0);

/// Separates every body overlapping the player, and reports how many.
///
/// Brute force, deliberately, and only against the player for now. It is O(N),
/// so at the default horde it is a thousand distance checks a tick and costs
/// nothing worth measuring. The uniform grid this eventually wants is an
/// *optimisation of something already correct* — which means it can be tested
/// by agreeing with this, and that test only exists if this exists first.
///
/// Gauss-Seidel: each correction is written immediately, so the next pair sees
/// it. That converges faster per pass than accumulating and applying at the
/// end, and its one real cost — the result depends on the order pairs are
/// visited — is fine here because the order is a fixed walk over storage rather
/// than anything that varies run to run. It is also why
/// [`crate::pass::remember`] copies rather than swapping two buffers.
pub(crate) fn separate(player: &mut Vec2, horde: &mut [Vec2], mut trace: TraceSink<'_>) -> usize {
    let contact = PLAYER_RADIUS + ENEMY_RADIUS;
    let mut contacts = 0;

    for (i, enemy) in horde.iter_mut().enumerate() {
        contacts += usize::from(pair(player, enemy, contact, PLAYER_INV_MASS, ENEMY_INV_MASS, i));
    }

    // Summarised, not per pair: a thousand overlapping bodies would otherwise
    // emit a thousand events a tick and flush the ring buffer of everything
    // rare enough to be worth keeping.
    if contacts > 0 {
        trace.emit(Event::Contacts { count: contacts });
    }

    contacts
}

/// Pushes two overlapping bodies apart along the line joining them, splitting
/// the correction between them by inverse mass. Returns whether they touched.
///
/// **Position projection, not an impulse.** There is no velocity here and no
/// momentum to conserve, which is the right model for a game whose movement is
/// deliberately instantaneous: a solver whose whole job is conserving momentum
/// would be fighting that. It also cannot inject energy, so there is no
/// restitution to zero out and no explosive pushback to suppress.
///
/// The overlap is corrected in **full**, in one pass. The usual advice is to
/// resolve a fraction — Box2D uses 0.2 — but that reasoning is about oblong
/// shapes overshooting as they rotate, and these are discs that do not rotate
/// and cannot stack. More to the point, a fraction applied once a tick is a
/// per-tick lerp toward zero overlap, which is frame-rate dependent in exactly
/// the way [`arpg_core::damp`] exists to prevent: it would converge five times
/// faster uncapped than under vsync, so pressing `V` would change how the game
/// feels and corrupt the measurement `V` is for. Full correction is exactly
/// dt-independent, and it keeps that question shut until the fixed timestep
/// makes it answerable.
fn pair(
    a: &mut Vec2,
    b: &mut Vec2,
    contact_distance: f32,
    a_inv_mass: f32,
    b_inv_mass: f32,
    tiebreak: usize,
) -> bool {
    let delta = *b - *a;
    let gap_sq = delta.length_squared();
    if gap_sq >= contact_distance * contact_distance {
        return false;
    }

    // Two immovable bodies have no correction to share out. Bailing keeps the
    // division below from being a zero-divide that quietly yields NaN.
    let share = a_inv_mass + b_inv_mass;
    if share <= 0.0 {
        return false;
    }

    let (normal, gap) = if gap_sq > COINCIDENT_SQ {
        let gap = gap_sq.sqrt();
        (delta / gap, gap)
    } else {
        (escape_direction(tiebreak), 0.0)
    };

    let overlap = contact_distance - gap;
    *a -= normal * (overlap * a_inv_mass / share);
    *b += normal * (overlap * b_inv_mass / share);
    true
}

/// A separation direction for two bodies occupying exactly the same point.
///
/// It has to come from somewhere, and it has to be the *same* somewhere every
/// run: a random direction would make two identical simulations diverge, which
/// is precisely what the determinism the fixed timestep is for would be
/// claiming. Deriving it from the pair's index costs nothing and is exactly
/// reproducible.
///
/// Golden-angle steps rather than a fixed direction, so a clump of coincident
/// bodies fans out instead of every one of them being pushed the same way and
/// re-stacking on the next tick.
fn escape_direction(tiebreak: usize) -> Vec2 {
    /// `PI * (3 - sqrt(5))`, written out because `sqrt` is not const.
    const GOLDEN_ANGLE: f32 = 2.399_963_2;

    let angle = tiebreak as f32 * GOLDEN_ANGLE;
    Vec2::new(angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moved here with the code it tests. A pass owns its constants, its const
    /// asserts and its tests; scattering them leaves the next person reading
    /// `separate` with no way to know what is already covered.
    /// What separation is *for*: afterwards the two are exactly touching, not
    /// merely less overlapped.
    #[test]
    fn separating_leaves_two_bodies_exactly_touching() {
        let mut a = Vec2::new(-0.1, 0.0);
        let mut b = Vec2::new(0.1, 0.0);

        assert!(pair(&mut a, &mut b, 1.0, 1.0, 1.0, 0));
        assert!((a.distance(b) - 1.0).abs() < 1e-5, "settled at {}", a.distance(b));
    }

    /// Bodies that are merely near must not be touched at all, or the solver
    /// would jitter everything within reach of everything.
    #[test]
    fn bodies_that_do_not_overlap_are_left_alone() {
        let mut a = Vec2::new(-1.0, 0.0);
        let mut b = Vec2::new(1.0, 0.0);

        assert!(!pair(&mut a, &mut b, 1.0, 1.0, 1.0, 0));
        assert_eq!((a, b), (Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0)));
    }

    /// **The case that produces NaN if it is not handled.** Two bodies at the
    /// same point have no line between them, and normalising their difference
    /// poisons a position beyond any clamp's ability to recover. It happens for
    /// real: a crowd converging on one target arrives at one point.
    #[test]
    fn coincident_bodies_separate_instead_of_producing_nan() {
        let mut a = Vec2::new(5.0, -3.0);
        let mut b = Vec2::new(5.0, -3.0);

        assert!(pair(&mut a, &mut b, 1.0, 1.0, 1.0, 7));

        assert!(a.is_finite() && b.is_finite(), "poisoned: {a} {b}");
        assert!((a.distance(b) - 1.0).abs() < 1e-5);
    }

    /// The escape direction must be reproducible, or two identical runs
    /// diverge the first time anything lands on top of anything else.
    #[test]
    fn the_escape_from_a_coincident_pair_is_deterministic() {
        let run = || {
            let (mut a, mut b) = (Vec2::ZERO, Vec2::ZERO);
            pair(&mut a, &mut b, 1.0, 1.0, 1.0, 42);
            (a, b)
        };
        assert_eq!(run(), run());

        // And neighbouring pairs must not all escape the same way, or a clump
        // separates into a line and re-stacks on the next tick.
        assert!(escape_direction(3).distance(escape_direction(4)) > 0.1);
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

        pair(&mut a, &mut b, 1.0, ia, ib, 0);
        assert!((centre(a, b) - before).length() < 1e-5);
    }

    /// Two immovable bodies cannot be pushed apart, and asking must not divide
    /// by their combined zero and yield NaN. Corpses and props will be exactly
    /// this case.
    #[test]
    fn two_immovable_bodies_are_left_where_they_are() {
        let mut a = Vec2::ZERO;
        let mut b = Vec2::new(0.1, 0.0);

        assert!(!pair(&mut a, &mut b, 1.0, 0.0, 0.0, 0));
        assert_eq!((a, b), (Vec2::ZERO, Vec2::new(0.1, 0.0)));
    }

    /// **The mechanic, at the level of one contact.** The heavy body barely
    /// moves; the light one does almost all the yielding. This is the whole of
    /// why a single enemy is a nudge rather than a wall.
    #[test]
    fn the_heavier_body_yields_less() {
        let mut player = Vec2::ZERO;
        let mut enemy = Vec2::new(0.5, 0.0);
        let (start_player, start_enemy) = (player, enemy);

        pair(&mut player, &mut enemy, 1.0, PLAYER_INV_MASS, ENEMY_INV_MASS, 0);

        let player_moved = player.distance(start_player);
        let enemy_moved = enemy.distance(start_enemy);
        assert!(
            enemy_moved > player_moved * 10.0,
            "expected a lopsided split, got {player_moved} vs {enemy_moved}"
        );
    }
}
