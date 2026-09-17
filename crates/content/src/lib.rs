//! Scene file decoding shared by the game and the headless gate.
//! Owns file I/O and RON decoding, not a second content schema. Scene and
//! template definitions stay in sim; gameplay descriptions stay in game.
//! Assertion scripts stay in the scenario binary. Both the app and runner use
//! this crate without depending on each other.

use std::path::Path;

/// Reads complete gameplay scenes using the game-owned schema.
/// Parsing and I/O complete before the caller attempts world mutation.
pub fn load_scene(path: &Path) -> Result<arpg_game::GameScene, LoadError> {
    let source = std::fs::read_to_string(path).map_err(LoadError::Read)?;
    parse_scene(&source).map_err(LoadError::Parse)
}

fn parse_scene(source: &str) -> Result<arpg_game::GameScene, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
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

/// Decodes a complete sim-owned placement for harness commands.
pub fn parse_placement(source: &str) -> Result<arpg_sim::Placed, ron::error::SpannedError> {
    ron::from_str(source)
}

/// Decodes a complete sim-owned source for harness commands.
pub fn parse_source(source: &str) -> Result<arpg_sim::SourceSpec, ron::error::SpannedError> {
    ron::from_str(source)
}

#[cfg(test)]
mod tests {
    use super::parse_scene;

    #[test]
    fn scenes_decode_the_game_owned_definition() {
        let scene = parse_scene(
            r#"(engine: (name: "trial", sources: [(pos: (0.0, 0.0), enabled: false)]))"#,
        ).unwrap();
        assert_eq!(scene.engine.name, "trial");
        assert!(!scene.engine.sources[0].enabled);
        assert!(scene.source_controls.is_empty());
    }

    #[test]
    fn scenes_reject_legacy_content_and_unknown_fields() {
        for source in [
            r#"(name: "trial", sources: [])"#,
            r#"(engine: (name: "game"), source_contorls: [])"#,
            r#"(engine: (name: "game", bodys: []))"#,
            r#"(engine: (name: "game"), source_controls: [(body: 0, source: "wrong")])"#,
        ] {
            assert!(parse_scene(source).is_err(), "invalid scene passed: {source}");
        }
    }
}
