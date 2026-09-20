//! Authoritative capsule used by Disintegrate damage and presentation.
use glam::Vec2;

/// A completed tick's beam, from the player to its maximum reach.
#[derive(Clone, Copy, Debug)]
pub struct Beam {
    start: Vec2,
    end: Vec2,
}

impl Beam {
    /// Reach in metres from the player centre.
    pub const LENGTH: f32 = 12.0;
    /// Half width in metres; enemy body radius also counts for contact.
    pub const RADIUS: f32 = 0.3;
    /// One damage point immediately, then every twelve ticks while held.
    pub const HIT_EVERY: u32 = 12;

    pub(crate) fn new(start: Vec2, facing: f32) -> Self {
        Self {
            start,
            end: start + Vec2::new(facing.sin(), facing.cos()) * Self::LENGTH,
        }
    }
    /// Ground-plane origin, in world X/Z.
    pub fn start(self) -> Vec2 {
        self.start
    }
    /// Ground-plane endpoint, in world X/Z.
    pub fn end(self) -> Vec2 {
        self.end
    }

    pub(crate) fn touches(self, centre: Vec2, radius: f32) -> bool {
        let axis = self.end - self.start;
        let t = ((centre - self.start).dot(axis) / axis.length_squared()).clamp(0.0, 1.0);
        centre.distance_squared(self.start + axis * t) <= (Self::RADIUS + radius).powi(2)
    }
}
const _: () = assert!(Beam::LENGTH > 0.0 && Beam::RADIUS > 0.0 && Beam::HIT_EVERY > 0);
