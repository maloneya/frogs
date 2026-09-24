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
    fn wall_files_share_typed_validation_and_reject_unknown_height() {
        for fields in [
            "centre: (0.0, 2.0), size: (3.0, 0.0)",
            "centre: (96.0, 2.0), size: (3.0, 0.2)",
            "centre: (0.0, 2.0), size: (3.0, 0.2), yaw: NaN",
            "centre: (0.0, 2.0), size: (3.0, 0.2), height: 2.0",
        ] {
            let source = format!("(engine: (name: \"bad wall\", walls: [({fields})]))");
            assert!(parse_scene(&source).is_err(), "{source}");
        }
        let content = parse_scene("(engine: (name: \"wall\", walls: [(centre: (0.0, 2.0), size: (3.0, 0.2))]))").unwrap();
        assert_eq!(content.engine.walls.len(), 1);
        assert!(arpg_game::Game::from_scene(&content).is_ok());
    }

    #[test]
    fn scenes_decode_the_game_owned_definition() {
        let scene = parse_scene(
            r#"(engine: (name: "trial", sources: [(pos: (0.0, 0.0), enabled: false)]))"#,
        ).unwrap();
        assert_eq!(scene.engine.name, "trial");
        assert!(!scene.engine.sources[0].enabled);
        assert!(scene.encounters.is_empty());
    }

    #[test]
    fn torch_files_use_shared_validation_before_game_mutation() {
        let valid = "(position: (1.0, 2.2, 3.0), colour: (1.0, 0.5, 0.2), intensity: 24.0, range: 14.0)";
        let scene = parse_scene(&format!("(engine: (name: \"lit\"), torches: [{valid}])")).unwrap();
        assert_eq!(scene.torches.len(), 1);
        for invalid in [
            valid.replace("1.0, 2.2, 3.0", "NaN, 2.2, 3.0"),
            valid.replace("1.0, 2.2, 3.0", "1.0, 0.1, 3.0"),
            valid.replace("1.0, 0.5, 0.2", "2.0, 0.5, 0.2"),
            valid.replace("intensity: 24.0", "intensity: -1.0"),
            valid.replace("intensity: 24.0", "intensity: inf"),
            valid.replace("range: 14.0", "range: 0.0"),
            valid.replace("range: 14.0", "range: 14.0, typo: 1"),
        ] {
            assert!(parse_scene(&format!("(engine: (name: \"invalid\"), torches: [{invalid}])")).is_err(), "{invalid}");
        }
    }

    #[test]
    fn set_piece_files_use_the_validated_game_vocabulary() {
        let valid = include_str!("../../../scenes/soul_trial.ron");
        assert_eq!(parse_scene(valid).unwrap().set_pieces[0].name(), "reveal_horde");
        for invalid in [
            valid.replace("duration_ticks: 15", "duration_ticks: 0"),
            valid.replace("duration_ticks: 15", "duration_ticks: 3601"),
            valid.replace("at: 0", "at: 4294967295"),
            valid.replace("(-0.5, 1.0, 0.3)", "(0.0, 0.0, 0.0)"),
            valid.replace("(-0.5, 1.0, 0.3)", "(NaN, 1.0, 0.0)"),
            valid.replace("fill: 0.12", "fill: 1.1"),
            valid.replace("intensity: 2.0", "intensity: inf"),
            valid.replace("colour: (0.55, 0.7, 1.0)", "colour: (-1.0, 0.7, 1.0)"),
            valid.replace("at: 0,", "at: 0, typo: true,"),
            valid.replace("fill: 0.12,", "fill: 0.12, typo: true,"),
            valid.replace("name: \"reveal_horde\"", "name: \"bad name\""),
            valid.replace("LightPulse", "UnknownAction"),
        ] {
            assert!(parse_scene(&invalid).is_err(), "invalid timeline admitted: {invalid}");
        }
    }

    #[test]
    fn scenes_reject_legacy_content_and_unknown_fields() {
        for source in [
            r#"(name: "trial", sources: [])"#,
            r#"(engine: (name: "game"), encoutners: [])"#,
            r#"(engine: (name: "game", bodys: []))"#,
            r#"(engine: (name: "game"), source_controls: [])"#,
        ] {
            assert!(parse_scene(source).is_err(), "invalid scene passed: {source}");
        }
    }
}
