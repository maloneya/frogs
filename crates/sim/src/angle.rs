//! Angles on the ground plane, wrapped to `-PI..=PI`.
//!
//! Shared rather than owned by the `face` pass, because the *drawing* of a
//! facing needs the same seam handling the turning of one does — `extract`
//! blends between two facings and would spin the body the long way round
//! without it. Two copies of this logic would be two chances to fix the seam in
//! one of them.

use core::f32::consts::{PI, TAU};

/// The angle successive indices should be spread by, in radians.
///
/// `PI * (3 - sqrt(5))`, written out because `sqrt` is not const.
///
/// **A stable substitute for randomness, and the reason it is shared.** Two
/// callers need a direction that has to come from *somewhere*, must be the same
/// somewhere every run, and must not agree with its neighbours: separating
/// bodies that sit on exactly the same point, and placing successive bodies
/// around a spawn ring. A random angle would make two identical simulations
/// diverge, and a fixed one would stack every body in the same direction and
/// re-stack them on the next tick. Golden-angle steps never repeat and never
/// clump, which is the property both want.
pub(crate) const GOLDEN_ANGLE: f32 = 2.399_963_2;

/// Folds an angle into `-PI..=PI`.
///
/// Applied after every turn, so a character spinning for an hour cannot walk
/// its facing out toward the precision limit.
pub(crate) fn wrap(radians: f32) -> f32 {
    let wrapped = radians.rem_euclid(TAU);
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

/// The shortest signed turn from one angle to another.
///
/// **Wraps the difference, not the endpoints.** Crossing the `±PI` branch cut
/// is a small step that naive subtraction reports as an almost-full revolution
/// the other way: `-3.13 - 3.13` is `-6.26`, when the real turn is `+0.02`.
pub(crate) fn shortest_arc(from: f32, to: f32) -> f32 {
    wrap(to - from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_way_round_the_seam_is_short() {
        let arc = shortest_arc(PI - 0.01, -PI + 0.01);
        assert!(arc.abs() < 0.05, "crossing the seam reported a {arc} radian turn");
        assert!(arc > 0.0, "the short way across the seam went the wrong direction");
    }

    #[test]
    fn wrapping_lands_inside_the_range() {
        for turns in -5..=5 {
            let a = wrap(0.3 + turns as f32 * TAU);
            assert!((a - 0.3).abs() < 1e-4, "{turns} turns of drift became {a}");
            assert!(a.abs() <= PI);
        }
    }
}
