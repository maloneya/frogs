//! Scene file decoding shared by the game and the headless gate.
//! Assertion scripts and reporting remain private to the scenario binary.

use std::path::Path;

/// Reads the simulation's scene vocabulary directly, with no mirror schema.
/// Parsing and I/O complete before the caller attempts world mutation.
pub fn load_scene(path: &Path) -> Result<arpg_sim::Scene, LoadError> {
    let source = std::fs::read_to_string(path).map_err(LoadError::Read)?;
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&source)
        .map_err(LoadError::Parse)
}

/// File errors remain distinguishable from syntax/schema errors.
#[derive(Debug)]
pub enum LoadError {
    /// The scene file could not be read.
    Read(std::io::Error),
    /// The content was not a valid scene description.
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(error) => write!(f, "read scene: {error}"),
            Self::Parse(error) => write!(f, "parse scene: {error}"),
        }
    }
}

impl std::error::Error for LoadError {}
