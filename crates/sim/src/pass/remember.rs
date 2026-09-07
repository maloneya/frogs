//! Snapshots where everything is, before anything moves it.

use glam::Vec2;

/// Copies this tick's starting state into the previous-tick slots.
///
/// **Every body, not just the ones this tick will touch.** A body left alone
/// must interpolate from where it is to where it is; a stale snapshot would
/// streak it back to wherever it last happened to move.
///
/// A flat copy rather than a swap of two buffers. A swap is cheaper and is
/// wrong the moment a pass reads a position it has already written this tick —
/// with a swap, `pos` would start the tick holding the tick-*before*-last's
/// values, and the Gauss-Seidel solver in [`crate::pass::separate`] reads
/// exactly that way.
pub(crate) fn remember(facing: f32, prev_facing: &mut f32, pos: &[Vec2], prev_pos: &mut [Vec2]) {
    *prev_facing = facing;
    prev_pos.copy_from_slice(pos);
}
