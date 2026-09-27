//! The borrowed presentation data submitted for one frame.

use crate::{
    CharacterHorde, CharacterPreview, DebugDisc, EffectVertex, MeshBatch, OrthoCamera, PointLight,
    Quad,
};

/// Borrowed geometry and uploaded assets for one frame. World units are metres;
/// colours are linear. The app has already resolved animation and visibility.
pub struct RenderFrame<'a> {
    /// The camera used for all world-space streams.
    pub camera: &'a OrthoCamera,
    /// Scene-wide ambient and directional illumination.
    pub environment: arpg_core::EnvironmentLight,
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
