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

use crate::ui::{Menu, Mode};
use arpg_gfx::{Glyphs, Quad, QuadSink};
use arpg_sim::{AttackProfile, AttackStatus};
use core::fmt::Write as _;
use glam::{Vec2, Vec4};

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
/// Viewport size bounds the scene list and external labels on small windows.
pub(crate) fn draw(
    font: &Glyphs,
    menu: &Menu,
    attack: AttackStatus,
    current: &str,
    viewport: Vec2,
    sink: &mut QuadSink<'_>,
) {
    if menu.mode() == Mode::Scenes {
        draw_picker(font, menu, current, viewport, sink);
        return;
    }
    // Stack-backed lines preserve the overlay's no-allocation frame path.
    let mut lines: [Text; 9] = core::array::from_fn(|_| Text::default());
    let (count, highlight) = if menu.open() {
        write!(lines[0], "ATTACK PROFILES").expect("line capacity");
        let selected = menu.profile(attack.profile);
        for (index, profile) in AttackProfile::ALL.into_iter().enumerate() {
            profile_line(&mut lines[index + 1], profile);
        }
        write!(
            lines[5],
            "Phase: {}   Elapsed: {} ticks   Struck: {}",
            attack.phase,
            attack.elapsed,
            attack.struck,
        )
        .expect("line capacity");
        write!(
            lines[6],
            "Arrows: choose profile   {}",
            if menu.pending().is_some() { "pending" } else { "applied" }
        )
        .expect("line capacity");
        write!(lines[7], "R: basic   F1/Esc: close   Selection affects next swing")
            .expect("line capacity");
        write!(lines[8], "Resolved values are captured when the swing begins")
            .expect("line capacity");
        let selected = AttackProfile::ALL
            .iter()
            .position(|profile| *profile == selected)
            .expect("selected profile belongs to the catalog");
        (9, Some(1 + selected))
    } else {
        write!(lines[0], "F1: attack profiles   F2: scenes").expect("line capacity");
        write!(
            lines[1],
            "{} / {} / tick {} / struck {}",
            attack.profile,
            attack.phase,
            attack.elapsed,
            attack.struck
        )
        .expect("line capacity");
        (2, None)
    };
    let lines = &lines[..count];
    let width = lines.iter().map(|line| font.measure(line.as_str())).fold(0.0, f32::max);
    draw_panel(font, lines, width, highlight, sink);
}

fn profile_line(line: &mut Text, profile: AttackProfile) {
    let resolved = profile.resolve();
    write!(
        line,
        "{:<12} {:>2}/{:>2}/{:>2} ticks  ({:>4.1},{:>3.1}) -> ({:>4.1},{:>3.1})  r {:.2}/{:.2}  kb {:.1}",
        profile.label(),
        resolved.startup(),
        resolved.active(),
        resolved.recovery().get(),
        resolved.start().x,
        resolved.start().y,
        resolved.end().x,
        resolved.end().y,
        resolved.start_radius(),
        resolved.end_radius(),
        resolved.knockback(),
    )
    .expect("line capacity");
}

/// A bounded window into the catalog, with no allocation or I/O per frame.
fn draw_picker(font: &Glyphs, menu: &Menu, current: &str, viewport: Vec2, sink: &mut QuadSink<'_>) {
    let width = (viewport.x - 2.0 * (MARGIN + PADDING)).min(900.0);
    let line_step = font.line_height() + 8.0;
    let available = ((viewport.y - 2.0 * (MARGIN + PADDING) + 8.0) / line_step).max(0.0) as usize;
    if available == 0 || width < font.measure("...") {
        return;
    }
    const VISIBLE_ROWS: usize = 6;
    const FIXED_ROWS: usize = 5; // Title, current scene, count, and two control hints.
    const ERROR_ROWS: usize = 2;
    let mut lines: [Text; VISIBLE_ROWS + FIXED_ROWS + ERROR_ROWS] =
        core::array::from_fn(|_| Text::default());
    let picker = menu.picker();
    let overhead = FIXED_ROWS + if picker.error().is_some() { ERROR_ROWS } else { 0 };
    let (count, highlight) = if available <= overhead {
        lines[0].label(font, "Enlarge window for scene picker. F2/Esc closes.", width);
        (1, None)
    } else {
        lines[0].label(font, "SCENE PLAYTESTS", width);
        write!(lines[1], "Current: ").expect("literal fits");
        lines[1].label(font, current, width);
        let range = picker.visible((available - overhead).min(VISIBLE_ROWS));
        let highlight = 2 + picker.selected() - range.start;
        let mut count = 2;
        for index in range.clone() {
            lines[count].label(font, picker.entries()[index].label(), width);
            count += 1;
        }
        write!(lines[count], "{}-{} of {}", range.start + 1, range.end, picker.entries().len())
            .expect("numbers fit");
        lines[count].fit(font, width);
        count += 1;
        if let Some(error) = picker.error() {
            lines[count].label(font, "Could not load. Current playtest continues.", width);
            lines[count + 1].label(font, error, width);
            count += 2;
        }
        lines[count].label(font, "Up/Down: select   Enter: start fresh", width);
        lines[count + 1].label(font, "F2/Esc: close   World keeps running", width);
        (count + 2, Some(highlight))
    };
    draw_panel(font, &lines[..count], width, highlight, sink);
}

/// Both menus submit the scrim, selection, and glyphs in the same order.
fn draw_panel(
    font: &Glyphs,
    lines: &[Text],
    width: f32,
    highlight: Option<usize>,
    sink: &mut QuadSink<'_>,
) {
    let line_step = font.line_height() + 8.0;
    let height = font.line_height() + (lines.len() - 1) as f32 * line_step;
    sink.push(Quad::solid(
        Vec4::new(MARGIN, MARGIN, width + 2.0 * PADDING, height + 2.0 * PADDING),
        PANEL,
    ));
    if let Some(row) = highlight {
        sink.push(Quad::solid(
            Vec4::new(
                MARGIN + PADDING / 2.0,
                MARGIN + PADDING + row as f32 * line_step - 3.0,
                width + PADDING,
                font.line_height() + 6.0,
            ),
            Vec4::new(0.018, 0.09, 0.13, 0.9),
        ));
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
    /// External names cannot inject rows, exceed stack capacity, or require a
    /// glyph outside the atlas. The real path stays untouched in the UI model.
    fn label(&mut self, font: &Glyphs, label: &str, width: f32) {
        for ch in label.chars() {
            if self.len == self.bytes.len() {
                self.len -= 3;
                self.write_str("...").expect("reserved suffix");
                break;
            }
            self.bytes[self.len] = if ch.is_ascii_graphic() || ch == ' ' { ch as u8 } else { b'?' };
            self.len += 1;
        }
        self.fit(font, width);
    }

    fn fit(&mut self, font: &Glyphs, width: f32) {
        if font.measure(self.as_str()) <= width {
            return;
        }
        while self.len > 0 && font.measure(self.as_str()) + font.measure("...") > width {
            self.len -= 1; // Picker text is ASCII, including every formatted literal.
        }
        self.write_str("...").expect("shortened text has suffix room");
    }

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

    #[test]
    fn picker_scrolls_and_bounds_external_text_to_the_viewport() {
        let font = Glyphs::system();
        let mut menu = Menu::default();
        let profile = arpg_sim::AttackProfile::default();
        menu.on_key(crate::ui::MenuKey::Scenes, profile);
        let long = "long scene\n名".repeat(40);
        menu.set_catalog(Ok((0..20)
            .map(|i| std::path::PathBuf::from(format!("{i}-{long}.ron")))
            .collect()));
        for _ in 0..100 {
            menu.on_key(crate::ui::MenuKey::Next, profile);
        }
        assert_eq!(menu.picker().selected(), 21);
        assert_eq!(menu.picker().visible(4), 18..22);
        menu.set_error("A file error\nwith a long path: ".repeat(50));
        for viewport in [Vec2::new(1280.0, 720.0), Vec2::new(500.0, 500.0), Vec2::new(180.0, 180.0)]
        {
            let mut buf = QuadBuffer::default();
            draw_picker(&font, &menu, &long, viewport, &mut buf.sink());
            let quads = buf.as_slice();
            let panel = quads[0].rect();
            assert!(panel.x + panel.z <= viewport.x && panel.y + panel.w <= viewport.y);
            for quad in &quads[1..] {
                let r = quad.rect();
                assert!(r.x >= panel.x && r.y >= panel.y, "{r} vs {panel}");
                assert!(
                    r.x + r.z <= panel.x + panel.z && r.y + r.w <= panel.y + panel.w,
                    "{r} vs {panel}"
                );
            }
        }
        assert_eq!(menu.picker().selected(), 21, "drawing never changes selection");
        assert!(menu.take_request().is_some(), "drawing cannot consume a request");
        let mut text = Text::default();
        text.label(&font, &long, 300.0);
        assert!(text.as_str().ends_with("..."));
        assert!(text.as_str().chars().all(|ch| ch.is_ascii_graphic() || ch == ' '));
        assert!(font.measure(text.as_str()) <= 300.0);
    }

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
            menu.on_key(crate::ui::MenuKey::Toggle, world.attack_status().profile);
            draw(
                &Glyphs::system(),
                &menu,
                world.attack_status(),
                "default",
                Vec2::new(1280.0, 720.0),
                &mut sink,
            );
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
