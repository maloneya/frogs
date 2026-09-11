//! Scene file decoding shared by the game and the headless gate.
//! Owns file I/O and RON decoding, not a second content schema. Scene and
//! template definitions stay in sim; gameplay descriptions stay in game.
//! Assertion scripts stay in the scenario binary. Both the app and runner use
//! this crate without depending on each other.

use std::path::Path;

/// Reads complete gameplay scenes or promotes legacy engine scenes.
/// Parsing and I/O complete before the caller attempts world mutation.
pub fn load_scene(path: &Path) -> Result<arpg_game::GameScene, LoadError> {
    let source = std::fs::read_to_string(path).map_err(LoadError::Read)?;
    parse_scene(&source).map_err(LoadError::Parse)
}

fn parse_scene(source: &str) -> Result<arpg_game::GameScene, ron::error::SpannedError> {
    let options = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    // Both schemas deny unknown fields. Legacy physical scenes are promoted;
    // neither decoder can discard a misspelled gameplay field.
    match options.from_str::<arpg_game::GameScene>(source) {
        Ok(scene) => Ok(scene),
        Err(game_error) => options.from_str::<arpg_sim::Scene>(source)
            .map(Into::into)
            .map_err(|engine_error| {
                // Prefer the decoder that reached further into the document.
                // Otherwise a typo in a legacy scene is hidden by GameScene's
                // rejection of its first valid field (usually `name`).
                if engine_error.span.end > game_error.span.end { engine_error }
                else { game_error }
            }),
    }
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

/// Decodes the sim-owned template vocabulary for harness commands.
/// New capabilities become reachable without a second hand-written field list.
pub fn parse_template(source: &str) -> Result<arpg_sim::Template, ron::error::SpannedError> {
    ron::from_str(source)
}

#[cfg(test)]
mod tests {
    use super::parse_scene;

    #[test]
    fn scene_schemas_preserve_their_own_parse_diagnostics() {
        let options = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        for source in [
            r#"(name: "legacy", bodys: [])"#,
            r#"(name: "legacy", sources: [(pos: (0.0, 0.0), enabled: 1)])"#,
        ] {
            assert_eq!(parse_scene(source).unwrap_err(),
                options.from_str::<arpg_sim::Scene>(source).unwrap_err());
        }
        for source in [
            r#"(engine: (name: "game"), source_contorls: [])"#,
            r#"(engine: (name: "game"), source_controls: [(body: 0, source: "wrong")])"#,
        ] {
            assert_eq!(parse_scene(source).unwrap_err(),
                options.from_str::<arpg_game::GameScene>(source).unwrap_err());
        }
    }

    #[test]
    fn both_file_schemas_reach_the_owning_definitions() {
        for source in [
            r#"(name: "trial", sources: [(pos: (0.0, 0.0), enabled: false)])"#,
            r#"(engine: (name: "trial", sources: [(pos: (0.0, 0.0), enabled: false)]))"#,
        ] {
            let scene = parse_scene(source).unwrap();
            assert_eq!(scene.engine.name, "trial");
            assert!(!scene.engine.sources[0].enabled);
            assert!(scene.source_controls.is_empty());
        }
        assert!(parse_scene(r#"(name: "legacy", source_controls: [])"#).is_err(),
            "legacy promotion cannot discard gameplay fields");
    }
}
