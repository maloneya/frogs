//! Validated ground-plane capsule geometry shared by effect producers and presentation.
use glam::Vec2;

/// A finite capsule with an optional clipping face. Authored reach and damage
/// cadence belong to its producer.
#[derive(Clone, Copy, Debug)]
pub struct Beam {
    start: Vec2,
    end: Vec2,
    end_normal: Option<Vec2>,
    radius: f32,
}

impl Beam {
    /// Construct a capsule within the shared geometric arithmetic domain.
    /// A zero-length axis produces no contacts.
    pub fn new(start: Vec2, end: Vec2, radius: f32) -> Result<Self, &'static str> {
        if !crate::valid_position(start)
            || !crate::valid_position(end)
            || !radius.is_finite()
            || radius <= 0.0
            || radius > crate::MAX_POSITION_COMPONENT
        {
            return Err(
                "beam geometry must fit the finite geometric domain with a positive radius",
            );
        }
        Ok(Self {
            start,
            end,
            end_normal: None,
            radius,
        })
    }
    /// Half width in world metres.
    pub fn radius(self) -> f32 {
        self.radius
    }
    pub(crate) fn clipped(mut self, hit: Option<crate::WallHit>) -> Self {
        if let Some(hit) = hit {
            self.end = hit.point();
            self.end_normal = Some(hit.normal());
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

    /// Boundary point of a circular body reached by this capsule.
    pub(crate) fn contact(self, centre: Vec2, radius: f32) -> Option<Vec2> {
        let axis = self.end - self.start;
        if axis.length_squared() == 0.0 {
            return None;
        }
        let t = ((centre - self.start).dot(axis) / axis.length_squared()).clamp(0.0, 1.0);
        let nearest = self.start + axis * t;
        if centre.distance_squared(nearest) > (self.radius + radius).powi(2) {
            return None;
        }
        Some(centre + (nearest - centre).normalize_or(-axis.normalize()) * radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsules_validate_the_complete_numeric_domain_and_zero_length_has_no_contacts() {
        for invalid in [f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(Beam::new(Vec2::splat(invalid), Vec2::Y, 1.0).is_err());
            assert!(Beam::new(Vec2::ZERO, Vec2::Y, invalid).is_err());
        }
        for invalid in [0.0, -1.0] {
            assert!(Beam::new(Vec2::ZERO, Vec2::Y, invalid).is_err());
        }
        let beam = Beam::new(Vec2::ZERO, Vec2::ZERO, 1.0).unwrap();
        assert!(beam.contact(Vec2::ZERO, 0.3).is_none());
        let extreme = Beam::new(
            Vec2::splat(-crate::MAX_POSITION_COMPONENT),
            Vec2::splat(crate::MAX_POSITION_COMPONENT),
            crate::MAX_POSITION_COMPONENT,
        )
        .unwrap();
        assert!(extreme.contact(Vec2::ZERO, 0.3).unwrap().is_finite());
    }
}
