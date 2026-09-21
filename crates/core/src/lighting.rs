//! Validated light vocabulary shared by authored scenes and the renderer.

use glam::Vec3;

/// Maximum live point lights, enforced at scene admission and GPU upload.
pub const MAX_POINT_LIGHTS: usize = 8;

/// Maximum simultaneous distant lights, reserved at scene admission.
pub const MAX_DIRECTIONAL_LIGHTS: usize = 8;

/// Distant illumination: a direction toward the source, with optional diffuse fill.
/// Linear colour and bounded intensity share the point-light validation door.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    toward_source: Vec3,
    colour: Vec3,
    intensity: f32,
    fill: f32,
}

impl DirectionalLight {
    /// Rejects non-finite or zero directions, invalid brightness and fill outside 0..=1.
    pub fn try_new(toward_source: Vec3, colour: Vec3, intensity: f32, fill: f32) -> Result<Self, &'static str> {
        let light = PointLight::try_new(Vec3::ZERO, colour, intensity, 1.0)?;
        if !toward_source.is_finite() || toward_source.abs().max_element() > 1_000_000.0 {
            return Err("light direction must be finite and bounded to one million");
        }
        let toward_source = toward_source.try_normalize().ok_or("light direction must be nonzero")?;
        if !fill.is_finite() || !(0.0..=1.0).contains(&fill) {
            return Err("light fill must be finite and between zero and one");
        }
        Ok(Self { toward_source, colour: light.colour(), intensity: light.intensity(), fill })
    }

    /// Unit direction from a surface toward the distant source.
    pub fn toward_source(self) -> Vec3 { self.toward_source }
    /// Linear emitted colour.
    pub fn colour(self) -> Vec3 { self.colour }
    /// Brightness before the surface-facing term.
    pub fn intensity(self) -> f32 { self.intensity }
    /// Fraction of illumination independent of surface orientation.
    pub fn fill(self) -> f32 { self.fill }
    /// Apply a finite fade in 0..=1 without allowing brightness amplification.
    pub fn faded(self, fraction: f32) -> Self {
        assert!(fraction.is_finite() && (0.0..=1.0).contains(&fraction), "invalid light fade");
        Self { intensity: self.intensity * fraction, ..self }
    }
}

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

#[cfg(test)]
mod directional_tests {
    use super::*;
    #[test]
    fn validates_direction_colour_intensity_fill_and_fade() {
        for (direction, colour, intensity, fill) in [
            (Vec3::ZERO, Vec3::ONE, 1.0, 0.0),
            (Vec3::splat(f32::NAN), Vec3::ONE, 1.0, 0.0),
            (Vec3::splat(f32::MAX), Vec3::ONE, 1.0, 0.0),
            (Vec3::Y, Vec3::splat(2.0), 1.0, 0.0),
            (Vec3::Y, Vec3::ONE, f32::INFINITY, 0.0),
            (Vec3::Y, Vec3::ONE, -1.0, 0.0),
            (Vec3::Y, Vec3::ONE, 1.0, -0.1),
            (Vec3::Y, Vec3::ONE, 1.0, f32::NAN),
        ] { assert!(DirectionalLight::try_new(direction, colour, intensity, fill).is_err()); }
        let light = DirectionalLight::try_new(Vec3::Y * 5.0, Vec3::ONE, 4.0, 0.0).unwrap();
        assert_eq!(light.toward_source(), Vec3::Y);
        assert_eq!(light.faded(0.25).intensity(), 1.0);
        for fade in [f32::NAN, -1.0, 1.1] {
            assert!(std::panic::catch_unwind(|| light.faded(fade)).is_err());
        }
    }
}
