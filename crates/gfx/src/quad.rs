//! Screen-space quads in physical pixels: origin top-left, Y downward.
//!
//! Solid quads sample the glyph atlas's white texel, sharing its pipeline and draw.

use arpg_core::LinearRgba;

use glam::Vec4;

use crate::text::WHITE_UV;

/// Capacity of the overlay buffer, in quads. Allocated once, up front, on the
/// same terms as [`arpg_core::MAX_INSTANCES`].
///
/// One glyph is one quad, so this is roughly "eight thousand characters on
/// screen at once" — far past the point a human could read any of it, and small
/// enough (384KB) that reserving it costs nothing worth measuring.
pub const MAX_QUADS: usize = 8192;

/// One screen-space rectangle with atlas UVs and linear RGBA.
///
/// ```compile_fail,E0308
/// arpg_gfx::Quad::solid(glam::Vec4::ZERO, glam::Vec4::ONE);
/// ```
///
/// ```compile_fail,E0277
/// let _: arpg_gfx::Quad = bytemuck::cast([0.0_f32; 12]);
/// ```
#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
pub struct Quad {
    /// `x, y, width, height`, in physical pixels from the top-left.
    rect: [f32; 4],
    /// `u0, v0, u1, v1` in `0..1` atlas space.
    uv: [f32; 4],
    /// Linear RGB plus unencoded alpha.
    color: [f32; 4],
}

const _: () = assert!(size_of::<Quad>() == 48);
const _: () = assert!(size_of::<Quad>() == 3 * 4 * size_of::<f32>());

impl Quad {
    /// A solid rectangle tinted in linear light.
    #[must_use]
    pub fn solid(rect: Vec4, color: LinearRgba) -> Self {
        Self { rect: rect.into(), uv: WHITE_UV, color: color.to_array() }
    }

    /// An atlas patch tinted with linear RGBA. The atlas owner supplies its UVs.
    #[must_use]
    pub(crate) fn textured(rect: Vec4, uv: [f32; 4], color: LinearRgba) -> Self {
        Self { rect: rect.into(), uv, color: color.to_array() }
    }

    /// Where this quad sits, as `x, y, width, height` in physical pixels.
    ///
    /// Read-only, for the reason [`arpg_core::Instance::pos`] gives: a test that
    /// cannot read back what crossed the seam can only assert that layout ran,
    /// not that it put anything in the right place.
    #[must_use]
    pub fn rect(&self) -> Vec4 {
        Vec4::from(self.rect)
    }

    /// Whether this quad samples the white texel — that is, whether it is a
    /// flat colour rather than a glyph.
    #[must_use]
    pub fn is_solid(&self) -> bool {
        self.uv == WHITE_UV
    }
}

/// The CPU-side staging buffer for one frame of overlay, allocated once at full
/// capacity. Hands out [`QuadSink`] and nothing else.
pub struct QuadBuffer {
    buf: Vec<Quad>,
}

impl Default for QuadBuffer {
    fn default() -> Self {
        Self { buf: Vec::with_capacity(MAX_QUADS) }
    }
}

impl QuadBuffer {
    /// Hands out the only writer there is, clearing last frame's contents on
    /// the way — the reset lives here rather than in the caller for the reason
    /// [`arpg_core::InstanceBuffer::sink`] spells out.
    pub fn sink(&mut self) -> QuadSink<'_> {
        self.buf.clear();
        QuadSink { remaining: MAX_QUADS, buf: &mut self.buf }
    }

    /// The frame's quads, in submission order — which is also **draw order**,
    /// since the overlay does not depth-test. A quad pushed later covers one
    /// pushed earlier.
    pub fn as_slice(&self) -> &[Quad] {
        &self.buf
    }
}

/// A write-only view of the overlay buffer that can push at most [`MAX_QUADS`]
/// and can do nothing else. The narrowness is the point; see
/// [`arpg_core::InstanceSink`], which this is a copy of on purpose.
pub struct QuadSink<'a> {
    buf: &'a mut Vec<Quad>,
    remaining: usize,
}

impl QuadSink<'_> {
    /// Silently drops anything past capacity.
    ///
    /// Dropping rather than panicking, because the thing most likely to overrun
    /// this is a debug readout printing more than it meant to, and a crash is a
    /// disproportionate answer to a long string. The cap is what stops it
    /// reaching the GPU buffer, which would truncate anyway and say nothing.
    pub fn push(&mut self, quad: Quad) {
        if self.remaining == 0 {
            return;
        }
        self.buf.push(quad);
        self.remaining -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The backstop no signature can provide: a producer that emits more than
    /// the GPU buffer was sized for.
    #[test]
    fn sink_stops_at_capacity() {
        let mut buf = QuadBuffer::default();
        let mut sink = buf.sink();
        for _ in 0..MAX_QUADS + 100 {
            sink.push(Quad::solid(Vec4::new(0.0, 0.0, 1.0, 1.0), LinearRgba::WHITE));
        }
        assert_eq!(buf.as_slice().len(), MAX_QUADS);
    }

    /// A second frame must not append to the first.
    #[test]
    fn sink_resets_between_frames() {
        let mut buf = QuadBuffer::default();
        for _ in 0..3 {
            let mut sink = buf.sink();
            sink.push(Quad::solid(Vec4::new(0.0, 0.0, 1.0, 1.0), LinearRgba::WHITE));
        }
        assert_eq!(buf.as_slice().len(), 1);
    }

    /// A solid quad must be distinguishable from a textured one *by its UVs*,
    /// because that is the only thing the shader can tell them apart by. If
    /// [`Quad::solid`] ever stopped naming the white texel, every bar would
    /// sample whichever glyph had been packed at the origin.
    #[test]
    fn a_solid_quad_names_the_white_texel() {
        assert!(Quad::solid(Vec4::new(0.0, 0.0, 4.0, 4.0), LinearRgba::WHITE).is_solid());
        assert!(!Quad::textured(Vec4::ZERO, [0.1, 0.1, 0.2, 0.2], LinearRgba::WHITE).is_solid());
    }

}
