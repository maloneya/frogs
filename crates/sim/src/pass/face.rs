//! Turns the body toward where it is walking.

use arpg_core::MoveDir;
use glam::Vec3;

use crate::angle;
use crate::Dt;

/// How fast the character turns, in radians per second.
///
/// Rate-limited where translation is not, and that asymmetry is the point: an
/// instantly-snapping facing reads as a body with no weight, while a slow one
/// makes the character feel like it is fighting the stick. This is the knob
/// that decides which.
const PLAYER_TURN_RATE: f32 = 14.0;
const _: () = assert!(PLAYER_TURN_RATE > 0.0);

/// Rotates `facing` toward the direction of travel by at most one tick's turn.
///
/// The step is clamped to the remaining arc, which is what makes the turn
/// frame-rate independent: without the clamp a coarse step overshoots the
/// target and a fine one does not, so the two disagree.
pub(crate) fn face(facing: &mut f32, dir: MoveDir, dt: Dt) {
    let dir = dir.as_vec3();
    if dir == Vec3::ZERO {
        // Standing still keeps the last facing. Snapping back to a default
        // would turn the character away from whatever it just walked up to,
        // the instant the key came up.
        return;
    }

    let arc = angle::shortest_arc(*facing, f32::atan2(dir.x, dir.z));
    let step = (PLAYER_TURN_RATE * dt.secs()).min(arc.abs());
    *facing = angle::wrap(*facing + step * arc.signum());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Accumulator;

    fn one_tick() -> Dt {
        Accumulator::default().pending(Dt::SECS).next().expect("one tick's worth buys one tick")
    }

    /// Predicted from `PLAYER_TURN_RATE`, for the reason spelled out in
    /// `pass::walk`: a hard-coded rate is invisible to every determinism check.
    ///
    /// Six ticks is 1.4 radians, short of the quarter turn toward +X, so this
    /// measures the rate rather than the arrival clamp.
    #[test]
    fn turning_covers_the_rate_it_claims() {
        let mut facing = 0.0;
        for _ in 0..6 {
            face(&mut facing, MoveDir::new(Vec3::X), one_tick());
        }

        let predicted = PLAYER_TURN_RATE * 6.0 * Dt::SECS;
        assert!(predicted < core::f32::consts::FRAC_PI_2, "the prediction ran past its target");
        assert!((facing - predicted).abs() < 1e-5, "turned {facing}, predicted {predicted}");
    }

    /// Without the clamp to the remaining arc, a coarse step overshoots and a
    /// fine one does not — which is frame-rate dependence wearing a disguise.
    #[test]
    fn turning_stops_on_target_rather_than_past_it() {
        let mut facing = 0.0;
        for _ in 0..60 {
            face(&mut facing, MoveDir::new(Vec3::X), one_tick());
        }
        assert!(
            (facing - core::f32::consts::FRAC_PI_2).abs() < 1e-6,
            "settled at {facing}, not the quarter turn it was aiming for"
        );
    }

    #[test]
    fn standing_still_keeps_the_last_facing() {
        let mut facing = 1.0;
        face(&mut facing, MoveDir::NONE, one_tick());
        assert_eq!(facing, 1.0);
    }
}
