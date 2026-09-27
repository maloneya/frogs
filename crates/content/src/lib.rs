//! Authoring documents and file I/O shared by authoring, game and headless tests.
//! Owns file I/O and RON decoding, not a second content schema. Scene and
//! template definitions stay in sim; gameplay descriptions stay in game.
//! Assertion scripts own their setups in the scenario binary; the app and
//! headless benchmark driver use this crate to load scene documents.

use std::path::Path;
mod document;
pub use document::{SceneDocument, SceneObjectKind, SceneObjectLabel, SceneObjectRef};

/// Read the playable scene from an authoring document, discarding its metadata.
/// Parsing and I/O complete before the caller attempts world mutation.
pub fn load_scene(path: &Path) -> Result<arpg_game::GameScene, LoadError> {
    let source = std::fs::read_to_string(path).map_err(LoadError::Read)?;
    parse_document(&source).map(|document| document.scene).map_err(LoadError::Parse)
}

#[cfg(test)]
fn parse_scene(source: &str) -> Result<arpg_game::GameScene, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
}

fn parse_document(source: &str) -> Result<SceneDocument, ron::error::SpannedError> {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME).from_str(source)
}

/// File errors remain distinguishable from syntax/schema errors.
#[derive(Debug)]
pub enum LoadError {
    /// The content file could not be read.
    Read(std::io::Error),
    /// The content did not match its owning schema.
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(error) => write!(f, "read content: {error}"),
            Self::Parse(error) => write!(f, "parse content: {error}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Decodes a complete game-owned placement for harness commands.
pub fn parse_placement(source: &str) -> Result<arpg_game::Placed, ron::error::SpannedError> {
    ron::from_str(source)
}

/// Decodes a complete game-owned source for harness commands.
pub fn parse_source(source: &str) -> Result<arpg_game::SourceSpec, ron::error::SpannedError> {
    ron::from_str(source)
}

#[cfg(test)]
mod tests {
    use super::parse_scene;

    #[test]
    fn source_decoding_and_direct_admission_share_validation() {
        for fields in [
            "pos: (NaN, 0.0)",
            "pos: (0.0, 0.0), radius: -1.0",
            "pos: (0.0, 0.0), radius: NaN",
            "pos: (0.0, 0.0), when: PlayerWithin(-1.0)",
            "pos: (0.0, 0.0), when: PlayerWithin(inf)",
            "pos: (96.0, 0.0), what: (physical: (fixed: true))",
            "pos: (0.0, 0.0), what: (physical: (fixed: true), actions: [Wait(ticks: 0)], behavior: Action(0))",
        ] {
            let input = format!("({fields})");
            let spec = super::parse_source(&input).unwrap();
            assert!(arpg_game::Source::try_from(spec).is_err(), "{input}");
            assert!(ron::from_str::<arpg_game::Source>(&input).is_err(), "{input}");
        }
        assert!(ron::from_str::<arpg_game::Source>("(pos: (30.0, 0.0), every: 0)").is_ok());
    }

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
    fn environment_files_validate_and_round_trip_at_the_owner() {
        let scene = parse_scene(r#"(engine: (name: "dusk"), environment: (
            ambient_colour: (0.1, 0.2, 0.3), ambient_intensity: 0.25,
            directional_colour: (0.9, 0.8, 0.7), directional_intensity: 0.75,
            direction: (-1.0, 2.0, 3.0),
        ))"#).unwrap();
        let encoded = ron::ser::to_string(&scene).unwrap();
        let decoded = parse_scene(&encoded).unwrap();
        assert_eq!(scene.environment, decoded.environment);
        for (colour, intensity, direction) in [
            ("(1.1, 0.0, 0.0)", "1.0", "(0.0, 1.0, 0.0)"),
            ("(1.0, 1.0, 1.0)", "-0.1", "(0.0, 1.0, 0.0)"),
            ("(1.0, 1.0, 1.0)", "1.0", "(0.0, 0.0, 0.0)"),
        ] {
            let invalid = format!(r#"(engine: (name: "bad"), environment: (
                ambient_colour: {colour}, ambient_intensity: {intensity},
                directional_colour: (1.0, 1.0, 1.0), directional_intensity: 0.0,
                direction: {direction},
            ))"#);
            assert!(parse_scene(&invalid).is_err());
        }
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
    fn scenes_reject_legacy_content_and_unknown_fields() {
        for source in [
            r#"(name: "trial", sources: [])"#,
            r#"(engine: (name: "game"), encoutners: [])"#,
            r#"(engine: (name: "game", bodys: []))"#,
            r#"(engine: (name: "game"), source_controls: [])"#,
            r#"(engine: (name: "game"), enemy_attacks: [])"#,
            r#"(engine: (name: "game", bodies: [(pos: (0.0, 0.0), what: (seeks: true))]))"#,
        ] {
            assert!(parse_scene(source).is_err(), "invalid scene passed: {source}");
        }
    }
}

/// Reads a complete runtime tuning file through sim's validated schema.
pub fn load_tuning(path: &Path) -> Result<arpg_sim::Tuning, LoadError> {
    let source = std::fs::read_to_string(path).map_err(LoadError::Read)?;
    parse_tuning(&source).map_err(LoadError::Parse)
}

/// Decodes complete settings without mutating a running game.
pub fn parse_tuning(source: &str) -> Result<arpg_sim::Tuning, ron::error::SpannedError> {
    ron::from_str(source)
}

/// Atomically replaces a tuning file; failed writes preserve its previous contents.
pub fn save_tuning(path: &Path, tuning: arpg_sim::Tuning) -> Result<(), String> {
    use std::io::Write;
    let text = ron::ser::to_string_pretty(&tuning, ron::ser::PrettyConfig::default())
        .map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("ron.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tuning_tests {
    use super::*;
    use arpg_sim::{Tuning, TuningField};

    #[test]
    fn complete_files_round_trip_and_invalid_inputs_are_rejected() {
        let tuning = Tuning::default().with(TuningField::SlamImpulse, 17.0).unwrap();
        let source = ron::ser::to_string(&tuning).unwrap();
        assert_eq!(parse_tuning(&source).unwrap(), tuning);
        for source in [
            "{}".to_string(),
            source.replace("SlamImpulse:17.0", "SlamImpulse:17.0,SlamImpulse:17.0"),
            source.replace("SlamImpulse:17.0", "SlamImpulse:NaN"),
            source.replace("SlamImpulse:17.0", "SlamImpulse:-1.0"),
            source.replace("SlamImpulse:17.0", "SlamImpulse:1001.0"),
            source.replace("SlamImpulse", "UnknownAttribute"),
        ] {
            assert!(parse_tuning(&source).is_err(), "{source}");
        }
    }
}

/// Read an authoring document with a revision for detecting external edits.
/// The revision includes comments and formatting as well as semantic content.
pub fn read_scene_document(path: &Path) -> Result<(SceneDocument, String), String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let scene = parse_document(&text).map_err(|error| error.to_string())?;
    scene.validate()?;
    Ok((scene, text_revision(&text)))
}

fn text_revision(text: &str) -> String {
    let mut hash = arpg_sim::Fnv::default();
    hash.usize(text.len());
    for byte in text.bytes() { hash.u64(u64::from(byte)); }
    format!("{:016x}", hash.finish())
}

/// Validate and atomically save a complete scene. A missing revision means a
/// new file; existing files require the revision obtained when opening them.
/// Leading line comments survive canonical serialization of the scene data.
/// Saves in the same directory serialize across processes. The advisory lock
/// protects cooperating scene writers; external editors do not take this lock.
pub fn save_scene_document(path: &Path, scene: &SceneDocument, revision: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    scene.validate()?;
    let path = std::path::absolute(path).map_err(|error| error.to_string())?;
    let path = path.as_path();
    // Lock the stable directory inode, not the file replaced by rename. Closing
    // this handle (including on error/process exit) releases the macOS lock.
    let directory = std::fs::File::open(path.parent().ok_or("Scene path needs a parent directory")?)
        .map_err(|error| error.to_string())?;
    directory.lock().map_err(|error| format!("Cannot lock scene directory: {error}"))?;
    let previous = match std::fs::read_to_string(path) {
        Ok(text) => {
            if revision != Some(text_revision(&text).as_str()) {
                return Err("Scene file changed or already exists. Reopen it or save under a new name.".into());
            }
            Some(text)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && revision.is_none() => None,
        Err(error) => return Err(format!("Cannot save scene: {error}")),
    };
    let header: String = previous.as_deref().unwrap_or("").lines()
        .take_while(|line| line.trim().starts_with("//") || line.trim().is_empty())
        .map(|line| format!("{line}\n")).collect();
    let text = header + &ron::ser::to_string_pretty(scene, ron::ser::PrettyConfig::default())
        .map_err(|error| error.to_string())? + "\n";
    let temporary = path.with_extension(format!("ron.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| -> Result<(), String> {
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        // Recheck after serialization/writing; refuse a changed baseline.
        let current = std::fs::read_to_string(path);
        let unchanged = match (&previous, &current) {
            (Some(old), Ok(current)) => old == current,
            (None, Err(error)) => error.kind() == std::io::ErrorKind::NotFound,
            _ => false,
        };
        if !unchanged { return Err("Scene changed during save; reopen before saving.".into()); }
        if previous.is_none() {
            // hard_link is create-only, so a concurrent new file cannot be lost.
            std::fs::hard_link(&temporary, path).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&temporary);
        } else { std::fs::rename(&temporary, path).map_err(|e| e.to_string())?; }
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result?;
    Ok(text_revision(&text))
}

#[cfg(test)]
mod scene_document_tests {
    use super::*;

    #[test]
    fn test_owned_scene_round_trips_through_the_owning_schema() {
        let original = parse_scene(r#"(
            engine: (name: "Round-trip test", bodies: [
                (pos: (1.0, 0.0), what: (role: Prop, physical: (fixed: true, interactable: true))),
                (pos: (5.0, 0.0), what: (physical: (fixed: true))),
            ], sources: [(pos: (0.0, 8.0), enabled: false,
                what: (role: Hostile, physical: (health: Some(3), character: true)))],
            walls: [(centre: (10.0, 0.0), size: (0.5, 4.0))],
            grids: [(origin: (-10.0, -10.0), columns: 2, rows: 2, spacing: 1.0)]),
            chests: [(body: 1)],
            encounters: [(name: "Round-trip trial", start: Activated(0),
                objective: Defeat((sources: [0], count: 1, within_ticks: 60)), on_success: [UnlockChest(0)])],
            trees: [(kind: Pine, position: (12.0, 3.0), yaw: 0.5, scale: 1.0)],
            torches: [(position: (0.0, 2.0, 0.0), colour: (1.0, 0.5, 0.2), intensity: 10.0, range: 8.0)],
        )"#).unwrap();
        let text = ron::ser::to_string_pretty(&original, ron::ser::PrettyConfig::default()).unwrap();
        let decoded = parse_scene(&text).unwrap();
        assert_eq!(arpg_game::Game::from_scene(&original).unwrap().hash(),
            arpg_game::Game::from_scene(&decoded).unwrap().hash());
    }

    #[test]
    fn scene_saves_refuse_conflicts_and_invalid_data_without_losing_file_or_header() {
        let path = std::env::temp_dir().join(format!("arpg-scene-save-{}.ron", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut scene = SceneDocument::default();
        scene.scene.engine.name = "Before".into();
        let revision = save_scene_document(&path, &scene, None).unwrap();
        assert!(save_scene_document(&path, &scene, None).is_err());
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("// Keep this playtest question.\n{original}")).unwrap();
        assert!(save_scene_document(&path, &scene, Some(&revision)).is_err());
        let (_, revision) = read_scene_document(&path).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        scene.scene.engine.grids.push(arpg_game::BodyGrid { origin: (0.0, 0.0), columns: 0, rows: 2, spacing: 1.0, what: arpg_game::ActorDefinition::passive_hostile() });
        assert!(save_scene_document(&path, &scene, Some(&revision)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        scene.scene.engine.grids.clear();
        scene.scene.engine.name = "After".into();
        let next = save_scene_document(&path, &scene, Some(&revision)).unwrap();
        assert_ne!(next, revision);
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("// Keep this playtest question.\n"));
        assert_eq!(load_scene(&path).unwrap().engine.name, "After");
        std::fs::remove_file(path).unwrap();
    }
}
