//! What the overlay says, and where.
//!
//! The counterpart to `World::extract`, one layer up. `extract` is the
//! simulation describing itself to a renderer in world terms; this is the
//! *app* describing a screen to the same renderer in pixels. Both hand over
//! nothing but geometry and colour, which is what lets `gfx` draw a readout
//! without ever learning what it reads out.
//!
//! It lives in `app` for the reason everything else here does: this is the only
//! crate that sees both sides. A health bar has to know what health is and
//! where the top-left of the window is, and those two facts live on opposite
//! sides of a boundary neither `sim` nor `gfx` may cross.
//!
//! **It is a pure function of what it is handed.** No state, no allocation, no
//! reading the clock. That is not stylistic: it makes the whole overlay
//! checkable without a GPU, because "what should be on screen" becomes a list
//! of quads a test can inspect — see the tests at the bottom, which never
//! create a device.

use arpg_gfx::{Glyphs, Quad, QuadSink};
use glam::Vec4;

/// Distance from the window edge to the panel, in physical pixels.
const MARGIN: f32 = 16.0;

/// Space between the panel's edge and the text inside it.
const PADDING: f32 = 10.0;

/// The readout's colour, linear. Bright enough to read over the pale floor and
/// the dark clear colour alike, which is the whole job of a debug readout and
/// the reason it is not simply white — white is what the floor already is.
const INK: Vec4 = Vec4::new(0.95, 0.95, 0.90, 1.0);

/// The panel behind the text, linear, with the alpha that makes it a scrim
/// rather than a box. Without it the readout is unreadable over the pale ground
/// tiles exactly when the player walks somewhere bright, which is a legibility
/// bug that only appears in half the world.
const PANEL: Vec4 = Vec4::new(0.004, 0.005, 0.008, 0.62);

/// Fills the frame's overlay.
///
/// Takes the metrics rather than a renderer, because placement depends on
/// nothing else — and a signature that says so is one a test can satisfy with
/// no window and no device.
///
/// **No viewport parameter yet, deliberately.** Everything here is anchored to
/// the top-left corner, so the window's size does not enter the arithmetic. The
/// moment something is centred or right-aligned it will, and taking the
/// argument before then would mean a parameter nobody could tell was unused.
pub(crate) fn draw(font: &Glyphs, sink: &mut QuadSink<'_>) {
    let text = "hello world";

    // The panel is pushed first because the overlay does not depth-test: draw
    // order *is* depth here, so anything that must appear behind must be
    // submitted before. This is the one ordering rule the whole layer has.
    let (width, height) = (font.measure(text), font.line_height());
    let panel = Vec4::new(MARGIN, MARGIN, width + PADDING * 2.0, height + PADDING * 2.0);
    sink.push(Quad::solid(panel, PANEL));

    font.layout(text, MARGIN + PADDING, MARGIN + PADDING, INK, sink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_gfx::QuadBuffer;

    /// **No GPU anywhere below.** `Glyphs` is the metrics table without the
    /// texture, and `draw` is a pure function, so the whole question "what
    /// would be on screen" is a list of rectangles a test can read — which is
    /// what stops the overlay being the one subsystem verifiable only by
    /// looking at it.
    fn overlay() -> QuadBuffer {
        let mut buf = QuadBuffer::default();
        {
            let mut sink = buf.sink();
            draw(&Glyphs::system(), &mut sink);
        }
        buf
    }

    /// The scrim must be submitted before the glyphs, or it paints over them.
    /// Nothing about the image would look *broken* if this reversed — the panel
    /// would simply be a solid rectangle, which is a plausible thing to be.
    #[test]
    fn the_panel_is_drawn_behind_the_text() {
        let buf = overlay();
        let quads = buf.as_slice();

        assert!(quads.len() > 1, "the overlay should draw a panel and some glyphs");
        assert!(quads[0].is_solid(), "the first quad must be the panel");
        assert!(
            quads[1..].iter().all(|q| !q.is_solid()),
            "everything after the panel must be a glyph"
        );
    }

    /// Every glyph must sit inside the panel. An off-by-one in the padding or a
    /// baseline mistake puts text over the panel's edge, which reads as sloppy
    /// rather than as broken and so survives being looked at.
    #[test]
    fn the_text_sits_inside_its_panel() {
        let buf = overlay();
        let quads = buf.as_slice();
        let panel = quads[0].rect();

        for glyph in &quads[1..] {
            let r = glyph.rect();
            assert!(r.x >= panel.x, "glyph starts left of the panel: {r} vs {panel}");
            assert!(r.y >= panel.y, "glyph starts above the panel: {r} vs {panel}");
            assert!(r.x + r.z <= panel.x + panel.z, "glyph runs past the right edge: {r}");
            assert!(r.y + r.w <= panel.y + panel.w, "glyph runs past the bottom edge: {r}");
        }
    }

    /// The readout clears the top-left corner and has area. A sign error in the
    /// margin puts it partly off the window — which a screenshot shows and
    /// nothing else would, since a quad off-screen is silently clipped.
    #[test]
    fn the_readout_clears_the_corner() {
        let buf = overlay();
        let panel = buf.as_slice()[0].rect();

        assert!(panel.x >= MARGIN && panel.y >= MARGIN, "the panel clears the corner: {panel}");
        assert!(panel.z > 0.0 && panel.w > 0.0, "the panel has area: {panel}");
    }
}
