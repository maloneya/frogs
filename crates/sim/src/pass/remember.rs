//! Snapshots where everything is, before anything moves it.

use glam::Vec2;

/// Snapshots every body, including stationary ones, before movement.
/// Copy rather than swap: in-place solvers need current positions as their input.
pub(crate) fn remember(pos: &[Vec2], prev_pos: &mut [Vec2]) {
    prev_pos.copy_from_slice(pos);
}
