//! Characters, as a texture and a table of where each one is in it.
//!
//! Rasterising a glyph — filling the bezier outline of a letter with coverage
//! values — is a solved problem with no engine insight left in it, so `fontdue`
//! does that part. What is *not* delegated is the atlas, the metrics table and
//! the layout, because those are the parts every other overlay feature reuses:
//! an icon sheet is this atlas with different contents, and a text run is this
//! layout with a different string.
//!
//! ## Why one texture rather than one per glyph
//!
//! A texture bind is a state change, and a state change between draws is the
//! thing that makes overlays expensive. Packed into one atlas, "hello world"
//! is eleven quads in one buffer drawn by one call with one bind — the same
//! argument the horde's instancing rests on, applied to text.
//!
//! ## The white texel
//!
//! [`WHITE`] is opaque white and no glyph is allowed to occupy it. That is
//! what lets [`crate::Quad::solid`] be part of this same draw: a health bar
//! is a quad whose UVs collapse onto that one texel, so it samples white
//! everywhere and comes out the colour it asked for. One pipeline draws text
//! and flat colour forever, and the cost of that is this one reserved pixel.
//!
//! The agreement is asserted at construction rather than trusted, because the
//! failure is silent: a glyph packed over that texel would make every solid bar
//! a smear of that letter, and nothing downstream could tell. And the UV that
//! names it is *derived* from it — [`WHITE_UV`] — so there is one definition
//! rather than two constants that can drift apart.

use glam::Vec4;

use crate::quad::{Quad, QuadSink};

/// Where the font comes from.
///
/// A system path rather than a vendored file, and that is defensible here for
/// exactly one reason: this engine is native macOS only, deliberately and
/// permanently, so "the font is present" is as safe an assumption as "Metal is
/// present". Monaco specifically because it is a plain single-face `.ttf` — no
/// `.ttc` collection to index into and no variable-font axes to instantiate —
/// and because it is monospaced, which makes a debug readout's columns line up
/// without a layout engine existing yet.
const SYSTEM_FONT: &str = "/System/Library/Fonts/Monaco.ttf";

/// The size glyphs are rasterised at, in **physical** pixels.
///
/// Physical rather than logical, because the overlay projects in physical
/// pixels — see [`crate::Quad`]. Rasterising at logical size and scaling up
/// is how text ends up soft on a retina display, and the softness is easy to
/// mistake for a sampling bug in the pipeline.
///
/// One size only. Multiple sizes means multiple atlases or one atlas with a
/// size axis, and neither buys anything until something wants text that is not
/// a readout.
const PX: f32 = 28.0;

/// The atlas is square and a power of two, which is nothing but habit — but
/// 512 wide is also a multiple of the 256-byte row alignment `write_texture`
/// demands, so the upload needs no padding pass.
const ATLAS: u32 = 512;
const _: () =
    assert!(ATLAS.is_multiple_of(256), "an atlas row must satisfy COPY_BYTES_PER_ROW_ALIGNMENT");

/// The texel reserved as opaque white, and the packer's first forbidden column.
///
/// **One definition, two readers.** The packer skips it and [`WHITE_UV`] points
/// at it, both derived from here — where before there were two constants
/// encoding one fact, in two crates, with nothing able to notice them
/// disagreeing. See the module docs for what that failure looks like.
const WHITE: (u32, u32) = (0, 0);

/// A degenerate UV rectangle over [`WHITE`]: every corner of a quad samples
/// exactly that texel, so the fetch is constant across the rectangle however
/// large it is. This is what [`crate::Quad::solid`] points at.
pub(crate) const WHITE_UV: [f32; 4] = {
    let (u, v) = (WHITE.0 as f32 / ATLAS as f32, WHITE.1 as f32 / ATLAS as f32);
    [u, v, u, v]
};

/// The characters the atlas holds: printable ASCII, `' '` through `'~'`.
///
/// Deliberately not "whatever the string contains, packed on demand". A
/// dynamic atlas needs eviction, repacking and a way to handle the frame where
/// the glyph is not ready yet, and none of that earns its place while the only
/// consumers are debug readouts and English item names.
const FIRST: char = ' ';
const LAST: char = '~';
const GLYPHS: usize = LAST as usize - FIRST as usize + 1;

/// What is drawn in place of a character the atlas does not hold.
///
/// A visible wrong answer rather than a silent gap: a run of `?` says "this
/// text contains something I cannot render", where skipping would quietly
/// shorten the string and look like a bug in whatever produced it.
const MISSING: char = '?';

/// One character's patch of the atlas, and how to place it.
#[derive(Clone, Copy, Default)]
struct Glyph {
    /// `u0, v0, u1, v1` into the atlas.
    uv: [f32; 4],
    /// Width and height of the patch, in pixels.
    size: [f32; 2],
    /// From the pen — which sits on the baseline — to the patch's top-left
    /// corner, y-down. Carries the bearing, which is what stops `p` sitting on
    /// the same line as `o` instead of hanging below it.
    offset: [f32; 2],
    /// How far the pen moves after drawing this character.
    advance: f32,
}

/// Where every character is in the atlas, and how to place it — **with no GPU
/// resource attached.**
///
/// The split from [`Font`] is the point of this type existing. Laying text out
/// needs metrics and nothing else; only *drawing* it needs a texture. Keeping
/// them apart means the question "what would be on screen" can be answered, and
/// asserted, with no device, no window and no adapter — so an overlay's layout
/// is testable on the same terms as the simulation rather than only by looking
/// at a screenshot.
pub struct Glyphs {
    glyphs: [Glyph; GLYPHS],
    /// Baseline to baseline, in pixels.
    line_height: f32,
    /// Top of the line to the baseline, in pixels. What turns the caller's
    /// "put it at y" into the baseline the glyph metrics are measured from.
    ascent: f32,
}

/// The rasterised font: the atlas texture, and the metrics describing it.
///
/// Built once at startup. Nothing here changes per frame, which is what makes
/// it shareable behind a `&` while the renderer holds the pipeline mutably.
pub(crate) struct Font {
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    glyphs: Glyphs,
}

/// One atlas, as the CPU makes it: the coverage bitmap, where each character
/// landed in it, and whether the reserved texel survived.
/// Rasterises the system font into an atlas bitmap and the metrics for it.
///
/// Panics if the font is missing or unparseable, which is the right response to
/// both: there is no sensible frame to draw without it, and a silent fallback
/// to no text would look exactly like the overlay pipeline being broken.
fn rasterise() -> (Vec<u8>, Glyphs) {
    let bytes =
        std::fs::read(SYSTEM_FONT).unwrap_or_else(|e| panic!("cannot read {SYSTEM_FONT}: {e}"));
    let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
        .unwrap_or_else(|e| panic!("cannot parse {SYSTEM_FONT}: {e}"));

    let mut pixels = vec![0u8; (ATLAS * ATLAS) as usize];
    let mut glyphs = [Glyph::default(); GLYPHS];

    // The reserved texel, written before anything is packed so the
    // assertion at the end has something to catch a packer bug with.
    pixels[(WHITE.1 * ATLAS + WHITE.0) as usize] = u8::MAX;

    // A shelf packer: fill a row left to right, drop to a new row when the
    // next glyph will not fit, and keep each row as tall as its tallest
    // occupant. It wastes the ragged space above short glyphs, which for
    // one font at one size is a few kilobytes of a 256KB texture — far
    // below the point where a real rectangle packer is worth its code.
    //
    // The first row starts past the reserved texel so nothing can land on
    // it; later rows start at 0, because the texel is behind them.
    let (mut x, mut y, mut shelf) = (WHITE.0 + 1, WHITE.1, 0u32);

    for (i, glyph) in glyphs.iter_mut().enumerate() {
        let ch = char::from_u32(FIRST as u32 + i as u32).expect("printable ASCII");
        let (metrics, coverage) = font.rasterize(ch, PX);
        let (w, h) = (metrics.width as u32, metrics.height as u32);

        if x + w > ATLAS {
            x = 0;
            y += shelf;
            shelf = 0;
        }
        assert!(y + h <= ATLAS, "the atlas is too small for {GLYPHS} glyphs at {PX}px");

        for row in 0..h {
            let from = (row * w) as usize;
            let to = ((y + row) * ATLAS + x) as usize;
            pixels[to..to + w as usize]
                .copy_from_slice(&coverage[from..from + w as usize]);
        }

        let atlas = ATLAS as f32;
        *glyph = Glyph {
            uv: [
                x as f32 / atlas,
                y as f32 / atlas,
                (x + w) as f32 / atlas,
                (y + h) as f32 / atlas,
            ],
            size: [metrics.width as f32, metrics.height as f32],
            // `xmin` is the bearing from the pen; `ymin` is the bitmap's
            // bottom edge measured *up* from the baseline, so the top edge
            // in a y-down space is minus the two together. Getting this
            // sign wrong does not fail anything — it simply puts every
            // letter the same distance from where it belongs, which reads
            // as "the text is a bit low" rather than as a bug.
            offset: [metrics.xmin as f32, -((metrics.height as i32 + metrics.ymin) as f32)],
            advance: metrics.advance_width,
        };

        x += w;
        shelf = shelf.max(h);
    }

    let line = font.horizontal_line_metrics(PX).expect("a horizontal font");

    // **Asserted at construction rather than reported as a flag.** A glyph
    // packed over the reserved texel makes every solid quad a smear of that
    // letter, and nothing downstream can tell. Failing here means it cannot be
    // observed false — and it fires in the real binary, not only under `cargo
    // test`, which a returned `bool` checked by a test could not manage.
    assert_eq!(
        pixels[(WHITE.1 * ATLAS + WHITE.0) as usize],
        u8::MAX,
        "a glyph was packed over the reserved white texel at {WHITE:?}"
    );

    (pixels, Glyphs { glyphs, line_height: line.new_line_size, ascent: line.ascent })
}

impl Font {
    /// Uploads the rasterised atlas to the GPU.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let (pixels, glyphs) = rasterise();

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d { width: ATLAS, height: ATLAS, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // One channel, because a glyph is coverage and nothing else. The
            // colour comes from the quad, which is what lets one atlas entry
            // serve every colour the same character is ever drawn in.
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS),
                rows_per_image: Some(ATLAS),
            },
            wgpu::Extent3d { width: ATLAS, height: ATLAS, depth_or_array_layers: 1 },
        );

        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            // **Nearest, not linear**, and that is a legibility decision rather
            // than a shortcut. Glyphs are rasterised at exactly the size they
            // are drawn, so every texel maps to one pixel and filtering has
            // nothing to interpolate but blur. It also makes the white texel
            // exact: linear filtering at uv (0,0) would blend it with whatever
            // is packed beside it, so a solid bar would come out slightly dim.
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("glyph sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            }),
            glyphs,
        }
    }

    /// Where the letters are, for anything laying text out rather than drawing
    /// it. See [`Glyphs`] for why the two are separable.
    pub(crate) fn glyphs(&self) -> &Glyphs {
        &self.glyphs
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub(crate) fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

}

impl Glyphs {
    /// Rasterises the system font's metrics, without touching a GPU.
    ///
    /// The door that makes an overlay's layout assertable headlessly. It does
    /// the same rasterisation [`Font::new`] does and throws the bitmap away,
    /// which costs a few milliseconds once and buys metrics that cannot
    /// disagree with the ones the renderer is using.
    #[must_use]
    pub fn system() -> Self {
        rasterise().1
    }

    /// Baseline to baseline, in physical pixels — what to add to `y` for the
    /// next line.
    #[must_use]
    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    /// How wide a string will be, in physical pixels.
    ///
    /// The width of its **widest line**, because [`Glyphs::layout`] returns the
    /// pen to the left margin on a `'\n'` and a measurement that did not would
    /// report a two-line readout as one line twice as wide — sizing every panel
    /// behind one far too large. The two walks have to agree about newlines or
    /// they are not measuring the same text.
    #[must_use]
    pub fn measure(&self, text: &str) -> f32 {
        text.split('\n')
            .map(|line| line.chars().map(|ch| self.glyph(ch).advance).sum())
            .fold(0.0, f32::max)
    }

    /// Pushes one quad per visible character, laid out from a top-left corner.
    ///
    /// **`(x, y)` is the top-left of the text**, not the baseline. A caller
    /// placing a readout in a corner is thinking in terms of the box it
    /// occupies, and asking it to know where a baseline sits would be exporting
    /// a typographic detail for no reason; the ascent is right here.
    ///
    /// A `'\n'` returns to `x` and drops a line. Nothing else is interpreted —
    /// no wrapping, no tabs, no bidi. Those are layout, this is placement, and
    /// the moment something needs a paragraph it needs a layout pass rather
    /// than more branches in here.
    pub fn layout(&self, text: &str, x: f32, y: f32, color: Vec4, sink: &mut QuadSink<'_>) {
        let mut pen = x;
        let mut baseline = y + self.ascent;

        for ch in text.chars() {
            if ch == '\n' {
                pen = x;
                baseline += self.line_height;
                continue;
            }

            let glyph = self.glyph(ch);

            // A space has an advance and no coverage. Pushing a zero-area quad
            // for it would be harmless and would still cost a vertex fetch and
            // a buffer slot, which at a few hundred characters is the
            // difference between the cap meaning what it says and not.
            if glyph.size[0] > 0.0 && glyph.size[1] > 0.0 {
                let rect = Vec4::new(
                    pen + glyph.offset[0],
                    baseline + glyph.offset[1],
                    glyph.size[0],
                    glyph.size[1],
                );
                sink.push(Quad::textured(rect, glyph.uv, color));
            }

            pen += glyph.advance;
        }
    }

    /// The atlas entry for a character, falling back to [`MISSING`].
    fn glyph(&self, ch: char) -> Glyph {
        let slot = (ch as usize)
            .checked_sub(FIRST as usize)
            .filter(|i| *i < GLYPHS)
            .unwrap_or(MISSING as usize - FIRST as usize);
        self.glyphs[slot]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quad::QuadBuffer;

    /// Metrics only: no device, no adapter, no window. Every test below but one
    /// runs on this, which is the property [`Glyphs`] exists to provide.
    fn glyphs() -> Glyphs {
        Glyphs::system()
    }

    fn laid_out(text: &str, x: f32, y: f32) -> QuadBuffer {
        let mut buf = QuadBuffer::default();
        {
            let mut sink = buf.sink();
            glyphs().layout(text, x, y, Vec4::ONE, &mut sink);
        }
        buf
    }

    /// The reserved texel's UV must point at the texel the packer actually
    /// reserved. Two constants encoding one fact is the drift this derivation
    /// exists to prevent, so this is what says the derivation is right.
    #[test]
    fn the_white_uv_names_the_reserved_texel() {
        let (u, v) = (WHITE_UV[0] * ATLAS as f32, WHITE_UV[1] * ATLAS as f32);
        assert_eq!((u as u32, v as u32), WHITE);
        assert_eq!(WHITE_UV[0], WHITE_UV[2], "the UV rectangle must be a single point");
        assert_eq!(WHITE_UV[1], WHITE_UV[3], "the UV rectangle must be a single point");

        // Construction asserts the texel is opaque, so simply building the
        // metrics exercises it — a glyph packed over the origin panics here.
        let _ = Glyphs::system();
    }

    /// `measure` and `layout` must agree about newlines. Before they did, a
    /// two-line readout measured as one line of both lines' width.
    #[test]
    fn measure_reports_the_widest_line() {
        let glyphs = glyphs();
        let one = glyphs.measure("aaaa");
        assert_eq!(glyphs.measure("aaaa\na"), one, "a short second line cannot widen the box");
        assert_eq!(glyphs.measure("a\naaaa"), one, "the widest line is the measurement");
    }

    /// Layout must produce one quad per *visible* character — spaces advance
    /// the pen and draw nothing — and the run must read left to right.
    #[test]
    fn a_string_lays_out_left_to_right_and_skips_blanks() {
        let buf = laid_out("ab cd", 10.0, 20.0);
        let quads = buf.as_slice();
        assert_eq!(quads.len(), 4, "one quad per visible character, none for the space");

        let xs: Vec<f32> = quads.iter().map(|q| q.rect().x).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "characters must advance rightward: {xs:?}");
        assert!(xs[0] >= 10.0, "the run starts at the x it was given: {xs:?}");

        // The gap across the space must exceed the gap between two adjacent
        // letters, which is what says the pen advanced for a character that
        // drew nothing rather than the space being dropped entirely.
        assert!(xs[2] - xs[1] > xs[1] - xs[0], "the space must still advance the pen: {xs:?}");
    }

    /// A newline returns to the starting column and drops exactly one line, so
    /// a multi-line readout stacks rather than running off the right edge.
    #[test]
    fn a_newline_returns_to_the_left_and_drops_one_line() {
        let buf = laid_out("a\na", 10.0, 20.0);
        let quads = buf.as_slice();
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].rect().x, quads[1].rect().x, "the second line starts at the same x");

        let dropped = quads[1].rect().y - quads[0].rect().y;
        let expected = glyphs().line_height();
        assert!(
            (dropped - expected).abs() < 0.01,
            "one newline should drop exactly one line: {dropped} vs {expected}"
        );
    }

    /// Text is placed by its top-left corner, so a caller asking for `(8, 8)`
    /// gets a box in the corner rather than one mostly above the window.
    #[test]
    fn text_is_placed_below_the_y_it_is_given() {
        let buf = laid_out("A", 0.0, 100.0);
        let top = buf.as_slice()[0].rect().y;
        assert!(top >= 100.0, "a capital must sit below the box top: {top}");
        assert!(top < 100.0 + glyphs().line_height(), "and inside the line: {top}");
    }

    /// `measure` and `layout` must walk the same metrics. If they drift, every
    /// centred or right-aligned readout is off by the difference.
    #[test]
    fn measure_agrees_with_what_layout_draws() {
        let buf = laid_out("hello", 0.0, 0.0);
        let rightmost =
            buf.as_slice().iter().map(|q| q.rect().x + q.rect().z).fold(f32::MIN, f32::max);
        let measured = glyphs().measure("hello");
        assert!(
            rightmost <= measured + 1.0,
            "drawn text ({rightmost}) must fit inside its measurement ({measured})"
        );
        assert!(measured > 0.0);
    }

    /// An unrepresentable character draws something rather than nothing, so a
    /// readout that acquires a `\u{b0}` reports its own limits instead of
    /// silently losing a column.
    #[test]
    fn a_character_outside_the_atlas_falls_back_visibly() {
        assert_eq!(laid_out("\u{e9}", 0.0, 0.0).as_slice().len(), 1);
    }
}
