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

use crate::ui::Menu;
use arpg_gfx::{Glyphs, Quad, QuadSink};
use arpg_sim::{AttackStatus, TICK_HZ};
use core::fmt::Write as _;
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
pub(crate) fn draw(font: &Glyphs, menu: &Menu, attack: AttackStatus, sink: &mut QuadSink<'_>) {
    // Stack-backed lines preserve the overlay's no-allocation frame path.
    let mut lines: [Text; 8] = core::array::from_fn(|_| Text::default());
    let count = if menu.open() {
        write!(lines[0], "ATTACK TUNING").expect("line capacity");
        let ticks = menu.recovery(attack.recovery).get();
        write!(
            lines[1],
            "Recovery  < {} ticks / {:.1} ms >",
            ticks,
            ticks as f32 * 1000.0 / TICK_HZ as f32
        )
        .expect("line capacity");
        write!(lines[2], "Phase: {}   Elapsed: {} ticks", attack.phase, attack.elapsed)
            .expect("line capacity");
        write!(lines[3], "Bodies struck: {}", attack.struck).expect("line capacity");
        if let Some(recovery) = attack.swing_recovery {
            write!(lines[4], "This swing's recovery: {} ticks", recovery.get())
                .expect("line capacity");
        } else {
            write!(lines[4], "This swing's recovery: --").expect("line capacity");
        }
        write!(
            lines[5],
            "{}",
            if menu.pending().is_some() {
                "Edit pending next tick; affects next swing."
            } else {
                "Edits affect the next swing."
            }
        )
        .expect("line capacity");
        write!(lines[6], "Left/Right: adjust   R: reset").expect("line capacity");
        write!(lines[7], "F1/Esc: close and play   World keeps running").expect("line capacity");
        8
    } else {
        write!(lines[0], "F1: attack tuning").expect("line capacity");
        write!(
            lines[1],
            "{} / tick {} / struck {}",
            attack.phase,
            attack.elapsed,
            attack.struck
        )
        .expect("line capacity");
        2
    };
    let lines = &lines[..count];
    let width = lines.iter().map(|line| font.measure(line.as_str())).fold(0.0, f32::max);
    let line_step = font.line_height() + 8.0;
    let height = font.line_height() + (count - 1) as f32 * line_step;
    let panel = Vec4::new(MARGIN, MARGIN, width + PADDING * 2.0, height + PADDING * 2.0);
    sink.push(Quad::solid(panel, PANEL));
    if menu.open() {
        let row = Vec4::new(
            MARGIN + PADDING / 2.0,
            MARGIN + PADDING + line_step - 3.0,
            width + PADDING,
            font.line_height() + 6.0,
        );
        sink.push(Quad::solid(row, Vec4::new(0.018, 0.09, 0.13, 0.9)));
    }
    for (index, line) in lines.iter().enumerate() {
        font.layout(
            line.as_str(),
            MARGIN + PADDING,
            MARGIN + PADDING + index as f32 * line_step,
            INK,
            sink,
        );
    }
}

/// Formatting storage with a loud bound, rather than silent truncation of a
/// number the designer is tuning. `write_str` accepts only complete UTF-8.
struct Text {
    bytes: [u8; 128],
    len: usize,
}

impl Default for Text {
    fn default() -> Self {
        Self { bytes: [0; 128], len: 0 }
    }
}

impl Text {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).expect("only write_str writes text")
    }
}

impl core::fmt::Write for Text {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len + text.len();
        let dest = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dest.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
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
            let world = arpg_sim::World::default();
            let mut menu = Menu::default();
            menu.on_key(crate::ui::MenuKey::Toggle, world.attack_status().recovery);
            draw(&Glyphs::system(), &menu, world.attack_status(), &mut sink);
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
            quads[1].is_solid() && quads[2..].iter().all(|q| !q.is_solid()),
            "the focused row must be behind all glyphs"
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
