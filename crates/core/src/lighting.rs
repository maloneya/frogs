//! Validated light vocabulary shared by authored scenes and the renderer.

use glam::Vec3;
use crate::LinearRgb;

/// Maximum live point lights, enforced at scene admission and GPU upload.
pub const MAX_POINT_LIGHTS: usize = 8;
const MAX_INTENSITY: f32 = 1_000_000.0;

/// A finite point light in world metres. Colours are linear, not sRGB.
/// Environment illumination is supplied separately; there is no visibility test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    position_range: [f32; 4],
    colour: LinearRgb,
    intensity: f32,
}

impl PointLight {
    /// Intensity controls brightness; range smoothly bounds its reach in metres.
    /// Invalid values are rejected here before they can reach either shader.
    pub fn new(position: Vec3, colour: LinearRgb, intensity: f32, range: f32) -> Self {
        Self::try_new(position, colour, intensity, range).expect("valid point light")
    }

    /// Rejects invalid input and bounds magnitudes to keep GPU arithmetic finite.
    /// Positions/ranges are bounded to one million metres, brightness to one
    /// million, and range is at least one centimetre.
    pub fn try_new(
        position: Vec3,
        colour: LinearRgb,
        intensity: f32,
        range: f32,
    ) -> Result<Self, &'static str> {
        if !position.is_finite() || position.abs().max_element() > 1_000_000.0 {
            return Err("light position must be finite and within one million metres");
        }
        validate_emission(colour, intensity)?;
        if !range.is_finite() || !(0.01..=1_000_000.0).contains(&range) {
            return Err("light range must be finite and between 0.01 and one million metres");
        }
        Ok(Self {
            position_range: position.extend(range).to_array(),
            colour,
            intensity,
        })
    }

    /// World position, also used to place the visible light source.
    pub fn position(self) -> Vec3 {
        Vec3::from_slice(&self.position_range)
    }

    /// Linear emitted colour, without intensity scaling.
    pub fn colour(self) -> LinearRgb {
        self.colour
    }

    /// Brightness scale in the shared attenuation equation.
    pub fn intensity(self) -> f32 {
        self.intensity
    }

    /// Scales presentation brightness, saturating at the shared emission limit.
    /// The authored light is unchanged; the factor must be finite and nonnegative.
    #[must_use]
    pub fn with_scaled_intensity(self, factor: f32) -> Self {
        assert!(factor.is_finite() && factor >= 0.0, "invalid light modulation");
        Self { intensity: (self.intensity * factor).min(MAX_INTENSITY), ..self }
    }

    /// Distance beyond which illumination is exactly zero.
    pub fn range(self) -> f32 {
        self.position_range[3]
    }
}

/// Scene-wide ambient fill and one directional light, independent of point lights.
/// Direction points from a surface toward the light; magnitude does not affect brightness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentLight {
    ambient_colour: LinearRgb,
    ambient_intensity: f32,
    directional_colour: LinearRgb,
    directional_intensity: f32,
    direction: Vec3,
}

impl EnvironmentLight {
    /// Validates linear RGB and finite bounded intensities. A nonzero direction
    /// is required even when its intensity is zero, so later edits remain safe.
    pub fn try_new(ambient_colour: LinearRgb, ambient_intensity: f32,
        directional_colour: LinearRgb, directional_intensity: f32, direction: Vec3,
    ) -> Result<Self, &'static str> {
        validate_emission(ambient_colour, ambient_intensity)?;
        validate_emission(directional_colour, directional_intensity)?;
        let length = direction.length_squared();
        if !direction.is_finite() || !(1e-12..=1e12).contains(&length) {
            return Err("light direction must be finite with length between 0.000001 and one million");
        }
        Ok(Self { ambient_colour, ambient_intensity, directional_colour, directional_intensity, direction })
    }

    /// Linear RGB ambient colour, without intensity scaling.
    pub fn ambient_colour(self) -> LinearRgb { self.ambient_colour }
    /// Uniform fill brightness for every surface direction.
    pub fn ambient_intensity(self) -> f32 { self.ambient_intensity }
    /// Linear RGB directional colour, without intensity scaling.
    pub fn directional_colour(self) -> LinearRgb { self.directional_colour }
    /// Directional brightness before the surface-facing term.
    pub fn directional_intensity(self) -> f32 { self.directional_intensity }
    /// Authored direction toward the light, normalized at GPU upload.
    pub fn direction(self) -> Vec3 { self.direction }

    /// Adds presentation brightness without exceeding the shared emission limit.
    /// Both boosts must be finite and nonnegative; authored settings are unchanged.
    #[must_use]
    pub fn with_added_intensity(self, ambient: f32, directional: f32) -> Self {
        assert!(ambient.is_finite() && ambient >= 0.0
            && directional.is_finite() && directional >= 0.0, "invalid light modulation");
        Self {
            ambient_intensity: (self.ambient_intensity + ambient).min(MAX_INTENSITY),
            directional_intensity: (self.directional_intensity + directional).min(MAX_INTENSITY),
            ..self
        }
    }
}

impl Default for EnvironmentLight {
    /// Readable neutral daylight. Darkness is an explicit authored choice.
    fn default() -> Self {
        Self::try_new(LinearRgb::WHITE, 0.22, LinearRgb::WHITE, 0.35, Vec3::new(0.4, 1.0, 0.3))
            .expect("valid daylight")
    }
}

fn validate_emission(colour: LinearRgb, intensity: f32) -> Result<(), &'static str> {
    if colour.to_vec3().max_element() > 1.0 {
        return Err("light colour components must not exceed one; use intensity for brightness");
    }
    if !intensity.is_finite() || !(0.0..=MAX_INTENSITY).contains(&intensity) {
        return Err("light intensity must be finite and between zero and one million");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_modulation_preserves_admitted_bounds_and_authored_lights() {
        let point = PointLight::new(Vec3::Y, LinearRgb::WHITE, MAX_INTENSITY, 10.0);
        assert_eq!(point.with_scaled_intensity(1.1), point);
        assert_eq!(point.with_scaled_intensity(f32::MAX), point);
        assert_eq!(point.with_scaled_intensity(0.5).intensity(), MAX_INTENSITY * 0.5);
        let environment = EnvironmentLight::try_new(LinearRgb::WHITE, MAX_INTENSITY,
            LinearRgb::WHITE, MAX_INTENSITY, Vec3::Y).unwrap();
        assert_eq!(environment.with_added_intensity(0.32, 1.7), environment);
        assert_eq!(environment.with_added_intensity(f32::MAX, f32::MAX), environment);
        for invalid in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(std::panic::catch_unwind(|| point.with_scaled_intensity(invalid)).is_err());
            assert!(std::panic::catch_unwind(|| environment.with_added_intensity(invalid, 0.0)).is_err());
            assert!(std::panic::catch_unwind(|| environment.with_added_intensity(0.0, invalid)).is_err());
        }
    }

    #[test]
    fn environment_rejects_nonfinite_brightness_and_unusable_direction() {
        let make = EnvironmentLight::try_new;
        for direction in [Vec3::ZERO, Vec3::splat(f32::NAN), Vec3::splat(f32::MAX), Vec3::splat(1e-20)] {
            assert!(make(LinearRgb::WHITE, 0.0, LinearRgb::WHITE, 0.0, direction).is_err());
        }
        for (colour, intensity) in [(LinearRgb::WHITE, -1.0), (LinearRgb::WHITE, f32::INFINITY), (LinearRgb::splat(1.1), 1.0)] {
            assert!(make(colour, intensity, LinearRgb::WHITE, 0.0, Vec3::Y).is_err());
            assert!(make(LinearRgb::WHITE, 0.0, colour, intensity, Vec3::Y).is_err());
        }
        assert!(make(LinearRgb::BLACK, 0.0, LinearRgb::BLACK, 0.0, Vec3::Y).is_ok());
    }

    #[test]
    fn rejects_invalid_light_parameters() {
        for (position, colour, intensity, range) in [
            (Vec3::splat(f32::NAN), LinearRgb::WHITE, 1.0, 10.0),
            (Vec3::ZERO, LinearRgb::splat(1.1), 1.0, 10.0),
            (Vec3::ZERO, LinearRgb::WHITE, -1.0, 10.0),
            (Vec3::ZERO, LinearRgb::WHITE, f32::NAN, 10.0),
            (Vec3::ZERO, LinearRgb::WHITE, 1.0, 0.0),
            (Vec3::ZERO, LinearRgb::WHITE, 1.0, f32::MAX),
        ] {
            assert!(
                std::panic::catch_unwind(|| PointLight::new(position, colour, intensity, range))
                    .is_err()
            );
        }
    }
}
