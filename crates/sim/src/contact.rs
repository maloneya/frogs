//! Disc-contact geometry shared by collision response and attack queries.

use glam::Vec2;

/// Below this separation two bodies have no line between them to push along,
/// and normalising their difference yields NaN — a position no clamp recovers.
/// Squared, since that is what the overlap test already has to hand.
const COINCIDENT_SQ: f32 = 1e-12;
const _: () = assert!(COINCIDENT_SQ > 0.0);

/// Minimum overlap counted as contact, above rounding noise to let settled crowds rest.
const SLOP: f32 = 1e-3;

/// Scale the noise floor with arena size; f32 precision decreases away from the origin.
const _: () = assert!(
    SLOP > crate::ARENA_HALF * f32::EPSILON * 16.0,
    "below the f32 noise floor at the arena's far corner, so settled contacts churn"
);
const _: () = assert!(SLOP < 0.01, "deep enough to see bodies resting inside one another");

/// Contact normal and penetration depth, independent of entity identity.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Contact {
    /// Unit vector pointing from the first body toward the second.
    normal: Vec2,
    /// How far the two interpenetrate, in world units.
    depth: f32,
}

impl Contact {
    /// Unit vector from the first body toward the second, including coincident bodies.
    pub(crate) fn normal(self) -> Vec2 {
        self.normal
    }

    /// How far the two interpenetrate, in world units. Positive for an overlap, zero for a predicted crossing.
    pub(crate) fn depth(self) -> f32 {
        self.depth
    }
}

/// Tests overlap beyond [`SLOP`]. `contact_distance` is the sum of radii.
/// `tiebreak` selects a stable escape direction for coincident centres.
pub(crate) fn between(a: Vec2, b: Vec2, contact_distance: f32, tiebreak: usize) -> Option<Contact> {
    let delta = b - a;
    let gap_sq = delta.length_squared();

    // The threshold is inset by the slop, and squared here so the common case
    // stays a multiply and a compare with no `sqrt`.
    let touching = contact_distance - SLOP;
    if gap_sq >= touching * touching {
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

/// Stable golden-angle escape direction so coincident pairs spread instead of restacking.
fn escape_direction(tiebreak: usize) -> Vec2 {
    let angle = tiebreak as f32 * crate::angle::GOLDEN_ANGLE;
    Vec2::new(angle.cos(), angle.sin())
}

/// Predicts a crossing that endpoint overlap resolution would see too late.
/// Ordinary approaching endpoint contacts remain the separation pass's job.
pub(crate) fn crossing(from: Vec2, relative_travel: Vec2, radius: f32) -> Option<Contact> {
    let from = from.as_dvec2();
    let relative_travel = relative_travel.as_dvec2();
    let radius = f64::from(radius);
    let travel_sq = relative_travel.length_squared();
    let projection = from.dot(relative_travel);
    if travel_sq == 0.0 || projection >= 0.0 { return None; }
    let closest = -projection / travel_sq;
    if closest >= 1.0 { return None; }
    let separation_sq = from.length_squared();
    if separation_sq <= radius * radius {
        return Some(Contact { normal: from.normalize().as_vec2(), depth: (radius - separation_sq.sqrt()) as f32 });
    }
    let discriminant = projection * projection - travel_sq * (separation_sq - radius * radius);
    if discriminant <= 0.0 { return None; }
    let impact = (-projection - discriminant.sqrt()) / travel_sq;
    let normal = (from + relative_travel * impact).normalize_or_zero().as_vec2();
    (normal != Vec2::ZERO).then_some(Contact { normal, depth: 0.0 })
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

    /// Check rounding noise at arena-scale coordinates, where f32 precision is poorest.
    #[test]
    fn a_pair_resting_a_whisker_inside_touching_is_not_a_contact() {
        let a = Vec2::new(90.0, -90.0);

        for steps in 1..=8 {
            // Walk down from exactly touching by single ulps.
            let mut distance = 1.0_f32;
            for _ in 0..steps {
                distance = f32::from_bits(distance.to_bits() - 1);
            }

            let b = a + Vec2::new(distance, 0.0);
            assert!(
                between(a, b, 1.0, 0).is_none(),
                "{steps} ulp inside touching reported a contact, which is the churn"
            );
        }
    }

    /// The slop must not swallow a real overlap. A body pushed a genuine
    /// fraction of its width into another is touching, and has to be reported.
    #[test]
    fn an_overlap_deeper_than_the_slop_is_still_a_contact() {
        let c = between(Vec2::ZERO, Vec2::new(0.99, 0.0), 1.0, 0);
        assert!(c.is_some(), "a hundredth of the contact distance is a real overlap");
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
