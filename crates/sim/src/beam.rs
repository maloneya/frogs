//! Authoritative capsule used by Disintegrate damage and presentation.
use glam::Vec2;

/// A completed tick's beam, from the player to its maximum reach.
#[derive(Clone, Copy, Debug)]
pub struct Beam {
    start: Vec2,
    end: Vec2,
    end_normal: Option<Vec2>,
}

impl Beam {
    /// Reach in metres from the player centre.
    pub const LENGTH: f32 = 12.0;
    /// Half width in metres; enemy body radius also counts for contact.
    pub const RADIUS: f32 = 0.3;
    /// Momentum added along the beam every touching simulation tick.
    pub const KNOCKBACK: f32 = 0.15;
    /// One damage point immediately, then every twelve ticks while held.
    pub const HIT_EVERY: u32 = 12;

    pub(crate) fn new(start: Vec2, facing: f32) -> Self {
        Self {
            start,
            end: start + Vec2::new(facing.sin(), facing.cos()) * Self::LENGTH,
            end_normal: None,
        }
    }
    pub(crate) fn clipped(mut self, walls: &crate::wall::Walls) -> Self {
        if let Some(hit) = walls.first_hit(self.start, self.end) {
            self.end = hit.point;
            self.end_normal = Some(hit.normal);
        }
        self
    }
    /// Ground-plane origin, in world X/Z.
    pub fn start(self) -> Vec2 {
        self.start
    }
    /// Ground-plane endpoint, in world X/Z.
    pub fn end(self) -> Vec2 {
        self.end
    }

    /// Outward ground-plane normal of the blocking face. Presentation uses it
    /// to trim the full beam width; a ray alone cannot describe a slanted cap.
    /// Zero means the origin is inside solid geometry and the beam has no length.
    pub fn end_normal(self) -> Option<Vec2> {
        self.end_normal
    }

    pub(crate) fn touches(self, centre: Vec2, radius: f32) -> bool {
        let axis = self.end - self.start;
        if axis.length_squared() == 0.0 {
            return false;
        }
        let t = ((centre - self.start).dot(axis) / axis.length_squared()).clamp(0.0, 1.0);
        centre.distance_squared(self.start + axis * t) <= (Self::RADIUS + radius).powi(2)
    }
}
const _: () = assert!(Beam::LENGTH > 0.0 && Beam::RADIUS > 0.0 && Beam::HIT_EVERY > 0 && Beam::KNOCKBACK > 0.0
        && Beam::KNOCKBACK.is_finite()
);
