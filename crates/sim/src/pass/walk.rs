//! Moves the player by its input.

use arpg_core::MoveDir;
use glam::{Vec2, Vec3Swizzles};

use crate::Dt;

/// Integrates one tick of movement.
///
/// **Instantaneous, deliberately.** Full speed on the first tick and a dead
/// stop on release. ARPG movement is essentially instant because
/// responsiveness beats momentum; acceleration is a feel knob, and one better
/// tuned against a fixed timestep than a variable one. *Turning* is
/// rate-limited — see [`crate::pass::face`] — translation is not.
///
/// `dir` is world-space and already unit-or-zero, because [`MoveDir`] is the
/// only door and it normalises. So this does not have to check.
pub(crate) fn walk(pos: &mut Vec2, dir: MoveDir, dt: Dt, speed: f32) {
    *pos += dir.as_vec3().xz() * speed * dt.secs();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Accumulator;
    use glam::Vec3;

    /// **Predicted from the constant, then asserted.** Determinism cannot catch
    /// a rate that is simply the wrong rate: replacing `dt.secs()` with a
    /// hard-coded number passes every replay test, because both runs then use
    /// the same wrong value and agree perfectly. Only a prediction from
    /// the selected tuning catches it.
    #[test]
    fn walking_covers_the_speed_it_claims() {
        let mut acc = Accumulator::default();
        let dt = acc.pending(Dt::SECS).next().expect("one tick's worth buys one tick");
        let east = MoveDir::new(Vec3::X);

        let mut pos = Vec2::ZERO;
        for _ in 0..6 {
            walk(&mut pos, east, dt, crate::Tuning::default().get(crate::TuningField::PlayerSpeed));
        }

        let predicted = crate::Tuning::default().get(crate::TuningField::PlayerSpeed) * 6.0 * Dt::SECS;
        assert!((pos.x - predicted).abs() < 1e-5, "walked {} in 6 ticks, predicted {predicted}", pos.x);
        assert_eq!(pos.y, 0.0, "walking along +X moved the other axis");
    }

    #[test]
    fn no_input_moves_nothing() {
        let mut acc = Accumulator::default();
        let dt = acc.pending(Dt::SECS).next().expect("one tick");

        let mut pos = Vec2::new(3.0, -4.0);
        walk(&mut pos, MoveDir::NONE, dt, 9.0);
        assert_eq!(pos, Vec2::new(3.0, -4.0));
    }
}
