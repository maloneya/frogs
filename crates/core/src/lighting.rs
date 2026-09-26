//! Validated light vocabulary shared by authored scenes and the renderer.

use glam::Vec3;

/// Maximum live point lights, enforced at scene admission and GPU upload.
pub const MAX_POINT_LIGHTS: usize = 8;

/// A finite point light in world metres. Colours are linear, not sRGB.
/// No ambient term or visibility test is added by the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    position_range: [f32; 4],
    colour_intensity: [f32; 4],
}

impl PointLight {
    /// Intensity controls brightness; range smoothly bounds its reach in metres.
    /// Invalid values are rejected here before they can reach either shader.
    pub fn new(position: Vec3, colour: Vec3, intensity: f32, range: f32) -> Self {
        Self::try_new(position, colour, intensity, range).expect("valid point light")
    }

    /// Rejects invalid input and bounds magnitudes to keep GPU arithmetic finite.
    /// Positions/ranges are bounded to one million metres, brightness to one
    /// million, and range is at least one centimetre.
    pub fn try_new(
        position: Vec3,
        colour: Vec3,
        intensity: f32,
        range: f32,
    ) -> Result<Self, &'static str> {
        if !position.is_finite() || position.abs().max_element() > 1_000_000.0 {
            return Err("light position must be finite and within one million metres");
        }
        if !colour.is_finite() || colour.min_element() < 0.0 || colour.max_element() > 1.0 {
            return Err("light colour components must be finite and between zero and one");
        }
        if !intensity.is_finite() || !(0.0..=1_000_000.0).contains(&intensity) {
            return Err("light intensity must be finite and between zero and one million");
        }
        if !range.is_finite() || !(0.01..=1_000_000.0).contains(&range) {
            return Err("light range must be finite and between 0.01 and one million metres");
        }
        Ok(Self {
            position_range: position.extend(range).to_array(),
            colour_intensity: colour.extend(intensity).to_array(),
        })
    }

    /// World position, also used to place the visible light source.
    pub fn position(self) -> Vec3 {
        Vec3::from_slice(&self.position_range)
    }

    /// Linear emitted colour, without intensity scaling.
    pub fn colour(self) -> Vec3 {
        Vec3::from_slice(&self.colour_intensity)
    }

    /// Brightness scale in the shared attenuation equation.
    pub fn intensity(self) -> f32 {
        self.colour_intensity[3]
    }

    /// Distance beyond which illumination is exactly zero.
    pub fn range(self) -> f32 {
        self.position_range[3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_light_parameters() {
        for (position, colour, intensity, range) in [
            (Vec3::splat(f32::NAN), Vec3::ONE, 1.0, 10.0),
            (Vec3::ZERO, Vec3::splat(-1.0), 1.0, 10.0),
            (Vec3::ZERO, Vec3::splat(f32::INFINITY), 1.0, 10.0),
            (Vec3::ZERO, Vec3::ONE, -1.0, 10.0),
            (Vec3::ZERO, Vec3::ONE, f32::NAN, 10.0),
            (Vec3::ZERO, Vec3::ONE, 1.0, 0.0),
            (Vec3::ZERO, Vec3::ONE, 1.0, f32::MAX),
        ] {
            assert!(
                std::panic::catch_unwind(|| PointLight::new(position, colour, intensity, range))
                    .is_err()
            );
        }
    }
}
