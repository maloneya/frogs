//! What the overlay says, and where.
//!
//! The screen-space counterpart of asset presentation: app describes the world
//! in mesh placements and a screen in quads. Both hand over geometry and colour,
//! so gfx draws the readout without learning what it reads out.
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

use crate::input::ATTACK_BINDINGS;
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

const HEALTH_FILL: Vec4 = Vec4::new(0.55, 0.035, 0.045, 1.0);
const ACTIVE_SLOT: Vec4 = Vec4::new(0.025, 0.18, 0.25, 1.0);
const SLOT: Vec4 = Vec4::new(0.025, 0.03, 0.04, 1.0);
const GAP: f32 = 4.0;
const BAR_HEIGHT: f32 = 8.0;
const PLAYER_PADDING: f32 = 6.0;
const PLAYER_TEXT_SCALE: f32 = 0.6;

fn player_panel_height(font: &Glyphs) -> f32 {
    4.0 * font.line_height() * PLAYER_TEXT_SCALE + BAR_HEIGHT + 3.0 * GAP + 4.0 * PLAYER_PADDING
}

/// Read-only health and fixed bindings. Highlight the committed swing, not the
/// next requested profile: pressing Slam during Cleave must not light up Slam.
pub(crate) fn draw_player(font: &Glyphs, attack: AttackStatus, health: u8, viewport: Vec2, sink: &mut QuadSink<'_>) {
    let height = player_panel_height(font);
    let minimum_top = MARGIN;
    let font = &font.scaled(PLAYER_TEXT_SCALE);
    let columns = ATTACK_BINDINGS.len() as f32;
    let widest_label = ATTACK_BINDINGS.iter()
        .map(|&(_, profile, binding)| font.measure(binding).max(font.measure(profile.label())))
        .fold(font.measure("Channeling"), f32::max);
    let gaps = (columns - 1.0) * GAP;
    let desired_width = columns * (widest_label + 2.0 * PLAYER_PADDING) + gaps + 2.0 * PLAYER_PADDING;
    let width = (viewport.x - 2.0 * MARGIN).min(desired_width);
    let top = viewport.y - MARGIN - height;
    let slot_width = (width - 2.0 * PLAYER_PADDING - gaps) / columns;
    let text_width = slot_width - 2.0 * PLAYER_PADDING;
    if text_width < font.measure("...") || top < minimum_top {
        return;
    }
    let left = (viewport.x - width) / 2.0;
    sink.push(Quad::solid(Vec4::new(left, top, width, height), PANEL));
    let mut hp = Text::default();
    write!(hp, "Health {health}/{}", arpg_game::PLAYER_MAX_HEALTH).expect("health fits");
    font.layout(hp.as_str(), left + PLAYER_PADDING, top + PLAYER_PADDING, INK, sink);
    let bar = Vec4::new(left + PLAYER_PADDING, top + PLAYER_PADDING + font.line_height() + GAP,
        width - 2.0 * PLAYER_PADDING, BAR_HEIGHT);
    sink.push(Quad::solid(bar, SLOT));
    if health > 0 {
        let fraction = f32::from(health.min(arpg_game::PLAYER_MAX_HEALTH)) / f32::from(arpg_game::PLAYER_MAX_HEALTH);
        sink.push(Quad::solid(Vec4::new(bar.x, bar.y, bar.z * fraction, bar.w), HEALTH_FILL));
    }
    let active = if attack.channeling { Some(AttackProfile::Disintegrate) } else { attack.swing_profile };
    let slot_top = bar.y + BAR_HEIGHT + GAP;
    for (index, &(_, profile, binding)) in ATTACK_BINDINGS.iter().enumerate() {
        let x = left + PLAYER_PADDING + index as f32 * (slot_width + GAP);
        let selected = active == Some(profile);
        sink.push(Quad::solid(Vec4::new(x, slot_top, slot_width,
            3.0 * font.line_height() + GAP + 2.0 * PLAYER_PADDING), if selected { ACTIVE_SLOT } else { SLOT }));
        let mut lines: [Text; 3] = core::array::from_fn(|_| Text::default());
        lines[0].label(font, binding, text_width);
        lines[1].label(font, profile.label(), text_width);
        if selected && attack.channeling {
            lines[2].label(font, "Channeling", text_width);
        } else if selected {
            write!(lines[2], "{}", attack.phase).expect("phase fits");
            lines[2].fit(font, text_width);
        } else {
            lines[2].label(font, if active.is_some() { "Busy" } else { "Ready" }, text_width);
        }
        for (row, line) in lines.iter().enumerate() {
            font.layout(line.as_str(), x + PLAYER_PADDING,
                slot_top + PLAYER_PADDING + row as f32 * font.line_height() + if row == 2 { GAP } else { 0.0 }, INK, sink);
        }
    }
}

/// Gameplay progress sits above the player HUD.
/// The number of rows is bounded by both game admission and viewport height.
pub(crate) fn draw_encounters<'a>(
    font: &Glyphs,
    encounters: impl Iterator<Item = (&'a str, arpg_game::EncounterState)>,
    viewport: Vec2,
    sink: &mut QuadSink<'_>,
) {
    let width = (viewport.x - 2.0 * (MARGIN + PADDING)).min(900.0);
    let line_step = font.line_height() + 8.0;
    let top = MARGIN;
    let bottom = viewport.y - MARGIN - player_panel_height(font) - GAP;
    let available = ((bottom - top - 2.0 * PADDING) / line_step).max(0.0) as usize;
    if available == 0 || width < font.measure("...") { return; }
    let mut lines: [Text; arpg_game::MAX_ENCOUNTERS] = core::array::from_fn(|_| Text::default());
    let mut count = 0;
    for (name, state) in encounters.take(available.min(lines.len())) {
        encounter_line(&mut lines[count], name, state);
        lines[count].fit(font, width);
        count += 1;
    }
    if count == 0 { return; }
    let lines = &lines[..count];
    let measured = lines.iter().map(|line| font.measure(line.as_str())).fold(0.0, f32::max);
    draw_panel(font, lines, measured, top, sink);
}

fn encounter_line(line: &mut Text, name: &str, state: arpg_game::EncounterState) {
    use arpg_game::EncounterPhase;
    let seconds = state.remaining_ticks.div_ceil(arpg_sim::TICK_HZ);
    let status = match state.phase {
        EncounterPhase::Ready => "Activate switch (E)",
        EncounterPhase::Running => "Defeat enemies",
        EncounterPhase::Succeeded => "Complete",
        EncounterPhase::Failed => "Time up",
        EncounterPhase::Orphaned => "Unavailable",
    };
    write!(line, "{name}: {}/{}  {seconds}s  {status}", state.defeated, state.target).expect("validated name and bounded numeric fields fit");
}

fn draw_panel(
    font: &Glyphs,
    lines: &[Text],
    width: f32,
    top: f32,
    sink: &mut QuadSink<'_>,
) {
    let line_step = font.line_height() + 8.0;
    let height = font.line_height() + (lines.len() - 1) as f32 * line_step;
    sink.push(Quad::solid(
        Vec4::new(MARGIN, top, width + 2.0 * PADDING, height + 2.0 * PADDING),
        PANEL,
    ));
    for (index, line) in lines.iter().enumerate() {
        font.layout(
            line.as_str(),
            MARGIN + PADDING,
            top + PADDING + index as f32 * line_step,
            INK,
            sink,
        );
    }
}

/// Formatting storage with a loud bound, rather than silent truncation of a
/// gameplay number. `write_str` accepts only complete UTF-8.
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
    /// glyph outside the atlas. Labels remain presentation-only.
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
            self.len -= 1; // HUD text is ASCII, including every formatted literal.
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
    fn encounter_hud_uses_authoritative_progress_and_rounds_remaining_seconds_up() {
        let mut line = Text::default();
        encounter_line(&mut line, "Trial", arpg_game::EncounterState {
            phase: arpg_game::EncounterPhase::Running, defeated: 17, target: 30, remaining_ticks: 61,
        });
        assert_eq!(line.as_str(), "Trial: 17/30  2s  Defeat enemies");
        let font = Glyphs::system();
        let state = arpg_game::EncounterState { phase: arpg_game::EncounterPhase::Succeeded,
            defeated: u32::MAX, target: u32::MAX, remaining_ticks: u32::MAX };
        let name = "N".repeat(48);
        for viewport in [Vec2::new(1280.0, 720.0), Vec2::new(400.0, 240.0)] {
            let mut buffer = QuadBuffer::default();
            draw_encounters(&font, std::iter::repeat_n((name.as_str(), state), arpg_game::MAX_ENCOUNTERS), viewport, &mut buffer.sink());
            for quad in buffer.as_slice() {
                let r = quad.rect();
                assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.z <= viewport.x && r.y + r.w <= viewport.y);
            }
        }
    }

    #[test]
    fn external_labels_are_bounded_and_cannot_inject_rows() {
        let font = Glyphs::system();
        let long = "long name\n名".repeat(40);
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
            let world = arpg_game::Game::default();
            draw_player(
                &Glyphs::system(),
                world.attack_status(),
                world.player_health(),
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
        assert!(!quads[1].is_solid(), "health text follows the panel");
        assert_eq!(quads.iter().filter(|q| q.is_solid()).count(), 6,
            "panel, health track/fill and three ability slots");
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

    /// The readout clears the window edges and has area. A sign error in the
    /// margin puts it partly off the window — which a screenshot shows and
    /// nothing else would, since a quad off-screen is silently clipped.
    #[test]
    fn the_readout_clears_the_corner() {
        let buf = overlay();
        let panel = buf.as_slice()[0].rect();

        assert!(panel.x >= MARGIN && panel.y >= MARGIN, "the panel clears the corner: {panel}");
        assert!(panel.z > 0.0 && panel.w > 0.0, "the panel has area: {panel}");
        assert!(
            panel.x + panel.z <= 1280.0 && panel.y + panel.w <= 720.0,
            "the complete attack catalog fits the default viewport: {panel}"
        );
    }
}
