//! Ground-plane angle wrapping shared by simulation and presentation.

use core::f32::consts::{PI, TAU};
use glam::Vec2;

/// Golden angle, `PI * (3 - sqrt(5))`, written out because sqrt is not const.
/// Spreads coincident contacts and ring spawns deterministically.
pub(crate) const GOLDEN_ANGLE: f32 = 2.399_963_2;

/// Folds an angle into `-PI..=PI`.
///
/// Applied after every turn, so a character spinning for an hour cannot walk
/// its facing out toward the precision limit.
pub fn wrap(radians: f32) -> f32 {
    let wrapped = radians.rem_euclid(TAU);
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

/// Local +X of something turned `yaw` about world up; yaw is clockwise from above.
pub(crate) fn axis(yaw: f32) -> Vec2 {
    let (sin, cos) = yaw.sin_cos();
    Vec2::new(cos, -sin)
}

/// The yaw that points local +Z along a ground-plane direction.
pub(crate) fn yaw_of(direction: Vec2) -> f32 {
    direction.x.atan2(direction.y)
}

/// The shortest signed turn from one angle to another.
///
/// **Wraps the difference, not the endpoints.** Crossing the `±PI` branch cut
/// is a small step that naive subtraction reports as an almost-full revolution
/// the other way: `-3.13 - 3.13` is `-6.26`, when the real turn is `+0.02`.
pub fn shortest_arc(from: f32, to: f32) -> f32 {
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
