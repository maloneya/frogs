//! A rectangle in screen space, and the buffer it travels in.
//!
//! The mirror of [`arpg_core::Instance`], one layer up. `Instance` is what the
//! *simulation* says outward — a thing in the world, at a position, in world
//! units. This is what an **overlay** says outward: a rectangle in pixels, over
//! the finished frame, answerable to the window rather than to the camera.
//!
//! What it shares with `Instance` is the shape of the seam: `gfx` draws quads
//! and must never learn that one of them is a health bar. A quad has a
//! rectangle, a patch of atlas and a colour, and there is deliberately nowhere
//! to write what it *means*.
//!
//! ## Why this is not in `arpg-core`
//!
//! Because it fails that crate's own bar, which is *needed by both, beholden to
//! neither*. `Instance` is there because `sim` must mint one without linking
//! the graphics stack; nothing that has to stay ignorant of wgpu ever mints a
//! quad. The two things that make them are [`crate::Glyphs::layout`] and the
//! app's own HUD, and the app already depends on this crate.
//!
//! Keeping it here is what makes two invariants free rather than aspirational:
//! [`Quad::textured`] can be `pub(crate)`, because the only caller that could
//! use it correctly is in this crate; and the white texel below can be *derived
//! from the atlas that guarantees it* rather than agreed with it across a crate
//! boundary no compiler spans.
//!
//! ## Why one type covers both text and solid colour
//!
//! A glyph and a health bar are the same draw: a rectangle sampling a texture.
//! The only difference is which texels. So [`Quad::solid`] points at a texel the
//! atlas guarantees is opaque white, and multiplying by it changes nothing —
//! one pipeline, one buffer, one draw call, for every overlay this engine will
//! ever have. Splitting them into a textured pipeline and an untextured one
//! would double the state changes to save a multiply.
//!
//! ## The coordinate system, stated once
//!
//! **Physical pixels, origin top-left, y increasing downward.** Not points, not
//! NDC, not y-up. That is winit's convention for a cursor position, which is
//! what makes hit-testing a click against a quad a comparison rather than a
//! conversion — and a conversion is where an off-by-a-scale-factor bug lives
//! that is invisible on a non-retina display.

use glam::Vec4;

use crate::text::WHITE_UV;

/// Capacity of the overlay buffer, in quads. Allocated once, up front, on the
/// same terms as [`arpg_core::MAX_INSTANCES`].
///
/// One glyph is one quad, so this is roughly "eight thousand characters on
/// screen at once" — far past the point a human could read any of it, and small
/// enough (384KB) that reserving it costs nothing worth measuring.
pub const MAX_QUADS: usize = 8192;

/// One screen-space rectangle on its way to the GPU.
///
/// 48 bytes, three `vec4`s, exactly like [`arpg_core::Instance`] — which is not a
/// coincidence but the same reasoning applied twice: a four-float alignment
/// keeps the vertex attribute arithmetic trivial, and the stride is a contract
/// with a `@location` list in WGSL that Rust cannot see across.
///
/// Fields are private because two of them are not free-form. `uv` must name a
/// real patch of the atlas, and `color` must be linear rather than sRGB; both
/// are things a struct literal could get wrong in a way that draws something
/// plausible. The constructors are the only doors.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Quad {
    /// `x, y, width, height`, in physical pixels from the top-left.
    rect: [f32; 4],
    /// `u0, v0, u1, v1` in `0..1` atlas space.
    uv: [f32; 4],
    /// Linear RGB plus alpha. See [`Quad::solid`] on why it is not sRGB.
    color: [f32; 4],
}

const _: () = assert!(size_of::<Quad>() == 48);
const _: () = assert!(size_of::<Quad>() == 3 * 4 * size_of::<f32>());

impl Quad {
    /// A rectangle of flat colour: a bar, a panel, a divider.
    ///
    /// **The colour is linear, not sRGB**, on the same terms as every colour in
    /// this engine: the surface is `Bgra8UnormSrgb` and the hardware encodes on
    /// write, so a value that looks correct written down comes out roughly five
    /// times too bright. There is no newtype guarding that yet, here or
    /// anywhere else, which is why it is said again.
    #[must_use]
    pub fn solid(rect: Vec4, color: Vec4) -> Self {
        Self { rect: rect.into(), uv: WHITE_UV, color: color.into() }
    }

    /// A rectangle sampling a patch of the atlas, for a glyph or an icon.
    ///
    /// The colour multiplies what is sampled, so a single white glyph in the
    /// atlas renders in any colour asked for — which is what keeps the atlas
    /// one entry per character rather than one per character per colour.
    ///
    /// The UV names a patch of the glyph atlas, so the only caller that can use
    /// it correctly is the one that packed it — [`crate::Glyphs::layout`].
    /// `pub(crate)` says exactly that, and says it to the compiler.
    #[must_use]
    pub(crate) fn textured(rect: Vec4, uv: [f32; 4], color: Vec4) -> Self {
        Self { rect: rect.into(), uv, color: color.into() }
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
            sink.push(Quad::solid(Vec4::new(0.0, 0.0, 1.0, 1.0), Vec4::ONE));
        }
        assert_eq!(buf.as_slice().len(), MAX_QUADS);
    }

    /// A second frame must not append to the first.
    #[test]
    fn sink_resets_between_frames() {
        let mut buf = QuadBuffer::default();
        for _ in 0..3 {
            let mut sink = buf.sink();
            sink.push(Quad::solid(Vec4::new(0.0, 0.0, 1.0, 1.0), Vec4::ONE));
        }
        assert_eq!(buf.as_slice().len(), 1);
    }

    /// A solid quad must be distinguishable from a textured one *by its UVs*,
    /// because that is the only thing the shader can tell them apart by. If
    /// [`Quad::solid`] ever stopped naming the white texel, every bar would
    /// sample whichever glyph had been packed at the origin.
    #[test]
    fn a_solid_quad_names_the_white_texel() {
        assert!(Quad::solid(Vec4::new(0.0, 0.0, 4.0, 4.0), Vec4::ONE).is_solid());
        assert!(!Quad::textured(Vec4::ZERO, [0.1, 0.1, 0.2, 0.2], Vec4::ONE).is_solid());
    }

    /// The rectangle survives the trip through the packed representation in the
    /// order the field names claim.
    #[test]
    fn a_quad_remembers_its_rectangle() {
        let quad = Quad::solid(Vec4::new(3.0, 5.0, 40.0, 12.0), Vec4::ONE);
        assert_eq!(quad.rect(), Vec4::new(3.0, 5.0, 40.0, 12.0));
    }
}
