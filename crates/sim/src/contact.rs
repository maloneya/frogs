//! Whether two bodies are touching, and along what line.
//!
//! **A query, not a pass.** It changes nothing and decides nothing; it reports
//! a geometric fact and hands it to whoever asked. That separation is the whole
//! point of the module existing, because the *response* to a touch is not one
//! thing:
//!
//! - separation pushes the pair apart along the normal,
//! - an attack's hitbox deals damage and pushes nobody,
//! - a trigger volume emits an event and does not even look at the depth,
//! - knockback wants the normal and throws the body along it.
//!
//! Those are four different passes over the same question. While the question
//! and one of its answers lived in the same function — as they did, in
//! `pass::separate::pair` — an attack could not *reuse* touching, only
//! re-implement it, and the second implementation is where the two quietly
//! stop agreeing about what "overlapping" means.
//!
//! ## Discs
//!
//! Every body is a disc, and the argument for that is in `crate::ENEMY_RADIUS`:
//! rotation-invariance, one unambiguous normal, and a test that is a squared
//! distance compare with no `sqrt` until an overlap is confirmed. What matters
//! here is that a disc's contact is *complete* — a normal and a depth say
//! everything there is to know about the overlap, so nothing downstream needs
//! to re-derive geometry from the positions.

use glam::Vec2;

/// Below this separation two bodies have no line between them to push along,
/// and normalising their difference yields NaN — a position no clamp recovers.
/// Squared, since that is what the overlap test already has to hand.
const COINCIDENT_SQ: f32 = 1e-12;
const _: () = assert!(COINCIDENT_SQ > 0.0);

/// Two bodies overlapping, described completely.
///
/// Carries no identity — not which bodies, not what kind. It is a fact about a
/// geometric relationship, which is what lets the same type serve a crowd
/// solver, a sword swing and a pressure plate. The caller already knows who it
/// asked about; putting that back in the answer would be a second copy of it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Contact {
    /// Unit vector pointing from the first body toward the second.
    normal: Vec2,
    /// How far the two interpenetrate, in world units.
    depth: f32,
}

impl Contact {
    /// Unit vector pointing from the first body toward the second.
    ///
    /// **Always unit length**, including when the two are exactly coincident —
    /// see [`escape_direction`]. A consumer may scale by it without checking,
    /// which is the whole reason the field is private: a `Contact` assembled by
    /// hand from an un-normalised difference would make every response that
    /// multiplies by it silently wrong by a factor nobody can see. Same
    /// argument, and the same shape, as `MoveDir`.
    pub(crate) fn normal(self) -> Vec2 {
        self.normal
    }

    /// How far the two interpenetrate, in world units. Always positive: a
    /// `Contact` that exists is an overlap that exists.
    pub(crate) fn depth(self) -> f32 {
        self.depth
    }
}

/// Reports whether two bodies overlap, and along what line.
///
/// `contact_distance` is the **sum** of the two radii — how far apart their
/// centres are when they are exactly touching. Summed by the caller rather than
/// taking two radii, because it is a loop invariant everywhere this is used and
/// recomputing it per body is arithmetic in the hot path for no reason.
///
/// `tiebreak` picks the escape direction when the two are exactly coincident.
/// It only has to be *stable* — see [`escape_direction`] for why it must not be
/// random, and why neighbouring values must not agree.
///
/// The `sqrt` happens only once an overlap is confirmed, which is what keeps
/// the negative case — overwhelmingly the common one — down to a multiply and a
/// compare.
pub(crate) fn between(a: Vec2, b: Vec2, contact_distance: f32, tiebreak: usize) -> Option<Contact> {
    let delta = b - a;
    let gap_sq = delta.length_squared();

    if gap_sq >= contact_distance * contact_distance {
        return None;
    }

    let (normal, gap) = if gap_sq > COINCIDENT_SQ {
        let gap = gap_sq.sqrt();
        (delta / gap, gap)
    } else {
        (escape_direction(tiebreak), 0.0)
    };

    // The only place a `Contact` is built. Everything about the invariant on
    // `normal` rests on that being true, so it is worth noticing if it stops
    // being: this function is the type's only constructor.
    Some(Contact { normal, depth: contact_distance - gap })
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

    /// Bodies that are merely near must not report a contact at all, or every
    /// consumer would act on everything within reach of everything.
    #[test]
    fn bodies_that_do_not_overlap_have_no_contact() {
        assert!(between(Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0), 1.0, 0).is_none());
    }

    /// Exactly touching is not overlapping. The boundary has to fall somewhere
    /// and it falls here, so that two bodies a solver has just separated do not
    /// immediately report contact again and jitter forever.
    #[test]
    fn bodies_exactly_touching_are_not_overlapping() {
        assert!(between(Vec2::ZERO, Vec2::new(1.0, 0.0), 1.0, 0).is_none());
    }

    /// The normal points from the first body toward the second, and the depth
    /// is how far in they are. A flipped normal compiles and separates bodies
    /// *into* each other.
    #[test]
    fn the_normal_points_from_the_first_body_to_the_second() {
        let c = between(Vec2::ZERO, Vec2::new(0.6, 0.0), 1.0, 0).expect("these overlap");

        assert!((c.normal() - Vec2::X).length() < 1e-6, "normal was {}", c.normal());
        assert!((c.depth() - 0.4).abs() < 1e-6, "depth was {}", c.depth());
    }

    /// Whichever way round it is asked, the two must agree about how deep the
    /// overlap is and disagree only about direction. A consumer that swaps its
    /// arguments must not get a different amount of overlap.
    #[test]
    fn asking_the_other_way_round_flips_only_the_normal() {
        let (a, b) = (Vec2::new(1.0, -2.0), Vec2::new(1.4, -1.7));
        let ab = between(a, b, 1.0, 0).expect("these overlap");
        let ba = between(b, a, 1.0, 0).expect("these overlap");

        assert!((ab.depth() - ba.depth()).abs() < 1e-6);
        assert!((ab.normal() + ba.normal()).length() < 1e-6, "normals were not opposite");
    }

    /// A contact's normal is unit length, so a consumer can multiply by it
    /// without normalising. Checked across a spread of directions rather than
    /// one, since a bug here would be a scale error rather than a wrong angle.
    #[test]
    fn the_normal_is_always_unit_length() {
        for i in 0..16 {
            let angle = i as f32 * 0.4;
            let b = Vec2::new(angle.cos(), angle.sin()) * 0.5;
            let c = between(Vec2::ZERO, b, 1.0, i).expect("half a unit apart overlaps");

            assert!((c.normal().length() - 1.0).abs() < 1e-5, "normal was {}", c.normal());
        }
    }

    /// **The case that produces NaN if it is not handled.** Two bodies at the
    /// same point have no line between them, and normalising their difference
    /// poisons a position beyond any clamp's ability to recover. It happens for
    /// real: a crowd converging on one target arrives at one point.
    #[test]
    fn coincident_bodies_get_a_finite_normal_instead_of_nan() {
        let at = Vec2::new(5.0, -3.0);
        let c = between(at, at, 1.0, 7).expect("coincident bodies are maximally overlapped");

        assert!(c.normal().is_finite(), "normal was {}", c.normal());
        assert!((c.normal().length() - 1.0).abs() < 1e-5);
        assert!((c.depth() - 1.0).abs() < 1e-5, "depth should be the whole contact distance");
    }

    /// The escape direction must be reproducible, or two identical runs
    /// diverge the first time anything lands on top of anything else.
    #[test]
    fn the_escape_from_a_coincident_pair_is_deterministic() {
        let run = || between(Vec2::ZERO, Vec2::ZERO, 1.0, 42).map(Contact::normal);
        assert_eq!(run(), run());

        // And neighbouring pairs must not all escape the same way, or a clump
        // separates into a line and re-stacks on the next tick.
        assert!(escape_direction(3).distance(escape_direction(4)) > 0.1);
    }
}
