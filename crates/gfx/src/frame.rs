//! The borrowed presentation data submitted for one frame.

use crate::{
    CharacterHorde, CharacterPreview, DebugDisc, EffectVertex, MeshBatch, OrthoCamera, PointLight,
    Quad,
};

/// Everything to draw in one frame, after the app has prepared presentation.
///
/// This borrows existing buffers and uploaded assets; assembling it neither
/// copies geometry nor allocates. [`crate::Renderer::render`] consumes the
/// description without retaining its borrows. It contains no simulation
/// identities, ticks or gameplay rules: interpolation, animation and visibility
/// choices have already been made by the caller.
///
/// World geometry uses metres and colours are linear. The renderer draws opaque
/// meshes and characters, then effects, diagnostic outlines and the screen-space
/// overlay. Capture requests are separate from this visual description.
///
/// Fields are public because the streams are independent; their element types
/// and renderer upload boundaries own validation and capacity limits. Every
/// stream is explicit, using an empty slice or `None` when absent.
pub struct RenderFrame<'a> {
    /// The camera used for all world-space streams.
    pub camera: &'a OrthoCamera,
    /// World-space point lights illuminating meshes and characters.
    pub lights: &'a [PointLight],
    /// Uploaded static meshes and their instance placements.
    pub meshes: &'a [MeshBatch<'a>],
    /// One independently posed character, if present.
    pub character: Option<CharacterPreview<'a>>,
    /// Instanced characters sharing the renderer's bounded pose buckets.
    pub horde: Option<CharacterHorde<'a>>,
    /// UI rectangles and glyphs in physical pixels, with a top-left origin.
    pub overlay: &'a [Quad],
    /// Always-visible world-space diagnostic outlines.
    pub debug_discs: &'a [DebugDisc],
    /// Depth-tested translucent world-space triangles.
    pub effects: &'a [EffectVertex],
}
