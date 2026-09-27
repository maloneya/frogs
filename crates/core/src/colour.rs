//! Explicit colour spaces at authoring and rendering boundaries.

use glam::Vec3;

/// Linear-light RGB, with finite nonnegative components; values above one are HDR.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgb([f32; 3]);

impl LinearRgb {
    /// Full reflectance in every channel.
    pub const WHITE: Self = Self([1.0; 3]);
    /// Zero reflectance in every channel.
    pub const BLACK: Self = Self([0.0; 3]);

    /// Constructs a linear colour, panicking on invalid components.
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        assert!(linear(r) && linear(g) && linear(b), "invalid linear RGB");
        Self([r, g, b])
    }

    /// Validates external linear colour components before admission.
    pub fn try_new(r: f32, g: f32, b: f32) -> Result<Self, &'static str> {
        if linear(r) && linear(g) && linear(b) {
            Ok(Self([r, g, b]))
        } else {
            Err("colour components must be finite and nonnegative")
        }
    }

    /// Equal linear intensity in all three channels.
    pub const fn splat(value: f32) -> Self {
        Self::new(value, value, value)
    }

    /// Linear components for serialization or GPU packing.
    pub const fn to_array(self) -> [f32; 3] {
        self.0
    }

    /// Linear components for arithmetic or reporting.
    pub fn to_vec3(self) -> Vec3 {
        Vec3::from_array(self.0)
    }

    /// Adds linear coverage, panicking unless alpha is finite and in `0..=1`.
    pub const fn with_alpha(self, alpha: f32) -> LinearRgba {
        LinearRgba::new(self.0[0], self.0[1], self.0[2], alpha)
    }

    /// Interpolates in linear light; the weight must be finite and in `0..=1`.
    pub fn lerp(self, other: Self, weight: f32) -> Self {
        assert!(unit(weight), "invalid colour interpolation weight");
        let rgb = self.to_vec3().lerp(other.to_vec3(), weight);
        Self::new(rgb.x, rgb.y, rgb.z)
    }

    /// Encodes for display using the sRGB transfer function, clipping HDR to one.
    pub fn to_srgb(self) -> Srgb {
        Srgb(self.0.map(|linear| {
            let linear = linear.min(1.0);
            if linear <= 0.003_130_8 {
                linear * 12.92
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            }
        }))
    }
}

/// Linear RGB and unencoded alpha; alpha is finite and in `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgba([f32; 4]);

impl LinearRgba {
    /// Opaque linear white.
    pub const WHITE: Self = Self([1.0; 4]);

    /// Constructs linear RGBA, panicking on invalid components.
    pub const fn new(r: f32, g: f32, b: f32, alpha: f32) -> Self {
        assert!(
            linear(r) && linear(g) && linear(b) && unit(alpha),
            "invalid linear RGBA"
        );
        Self([r, g, b, alpha])
    }

    /// Validates external linear RGBA before admission.
    pub fn try_new(r: f32, g: f32, b: f32, alpha: f32) -> Result<Self, &'static str> {
        LinearRgb::try_new(r, g, b)?;
        if !unit(alpha) {
            return Err("alpha must be finite and between zero and one");
        }
        Ok(Self([r, g, b, alpha]))
    }

    /// Linear components and unencoded alpha for GPU packing.
    pub const fn to_array(self) -> [f32; 4] {
        self.0
    }
}

/// sRGB-encoded RGB, with finite components in `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Srgb([f32; 3]);

impl Srgb {
    /// Encoded RGB from an eight-bit image or colour picker.
    pub fn from_rgb8(rgb: [u8; 3]) -> Self {
        Self(rgb.map(|channel| f32::from(channel) / 255.0))
    }

    /// Quantizes encoded RGB to eight bits per channel.
    pub fn to_rgb8(self) -> [u8; 3] {
        self.0
            .map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
    }

    /// Decodes sRGB before lighting, tinting or blending.
    pub fn to_linear(self) -> LinearRgb {
        LinearRgb(self.0.map(|encoded| {
            if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            }
        }))
    }
}

const fn linear(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}

const fn unit(value: f32) -> bool {
    value >= 0.0 && value <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_decode_and_encode_preserve_all_byte_values() {
        for value in 0..=255 {
            assert_eq!(
                Srgb::from_rgb8([value; 3]).to_linear().to_srgb().to_rgb8(),
                [value; 3]
            );
        }
        let decoded = Srgb::from_rgb8([10, 11, 128]).to_linear().to_array();
        for (actual, expected) in decoded
            .into_iter()
            .zip([0.003_035_27, 0.003_346_54, 0.215_860_5])
        {
            assert!((actual - expected).abs() < 1e-7);
        }
        assert_eq!(LinearRgb::splat(0.5).to_srgb().to_rgb8(), [188; 3]);
        assert_eq!(LinearRgb::BLACK.to_srgb().to_rgb8(), [0; 3]);
        assert_eq!(LinearRgb::WHITE.to_srgb().to_rgb8(), [255; 3]);
    }

    #[test]
    fn rejects_invalid_components_and_preserves_alpha() {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01] {
            assert!(LinearRgb::try_new(invalid, 0.0, 1.0).is_err());
            assert!(LinearRgb::try_new(0.0, invalid, 1.0).is_err());
            assert!(LinearRgb::try_new(0.0, 1.0, invalid).is_err());
            assert!(LinearRgba::try_new(0.0, 1.0, 0.0, invalid).is_err());
            assert!(std::panic::catch_unwind(|| LinearRgb::new(invalid, 0.0, 1.0)).is_err());
            assert!(std::panic::catch_unwind(|| LinearRgb::WHITE.with_alpha(invalid)).is_err());
        }
        assert!(LinearRgba::try_new(0.0, 1.0, 0.0, 1.01).is_err());
        assert_eq!(
            LinearRgb::new(10.0, 0.0, 0.0).to_srgb().to_rgb8(),
            [255, 0, 0]
        );
        let rgb = Srgb::from_rgb8([128; 3]).to_linear();
        assert_eq!(
            rgb.with_alpha(0.25).to_array(),
            [
                rgb.to_array()[0],
                rgb.to_array()[1],
                rgb.to_array()[2],
                0.25
            ]
        );
    }
}
