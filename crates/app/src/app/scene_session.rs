//! Local scene selection survives launches without entering deterministic gameplay.
use super::App;
use std::path::{Path, PathBuf};

impl App {
    pub(super) fn restore_scene_session(&mut self) {
        // Automated playtests must neither inherit nor overwrite the scene a
        // person last played. An explicit path lets harness tests opt in safely.
        self.scene_session_path = std::env::var_os("ARPG_SESSION_FILE").map(PathBuf::from)
            .or_else(|| std::env::var_os("ARPG_HARNESS").is_none().then(|| "target/last-scene.json".into()));
        self.restore_scene_file();
    }

    fn restore_scene_file(&mut self) {
        let Some(path) = self.scene_session_path.clone() else { return; };
        let result = (|| {
            let data = match std::fs::read(&path) {
                Ok(data) => data,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
            };
            let scene: PathBuf = serde_json::from_slice(&data).map_err(|error| format!("Invalid scene preference: {error}"))?;
            self.start_scene_file(&scene)?;
            Ok(true)
        })();
        match result {
            Ok(false) if !self.companion_error => self.companion_result(&Ok(
                "Choose a scene in Scenes, or create a new one. Save & play starts it.".into())),
            Ok(_) => {},
            Err(error) => {
                // Tuning loads before scene restoration. Keep its failure visible
                // even when a separate scene preference also needs attention.
                let prior = if self.companion_error { format!("{}\n", self.companion_message) } else { String::new() };
                self.companion_result(&Err(format!("{prior}{error}. Choose another scene in Scenes.")));
            }
        }
    }

    /// A preference write cannot roll back a successfully installed playtest.
    pub(super) fn remember_scene_file(&self, scene: &Path) {
        let Some(path) = &self.scene_session_path else { return; };
        let write = || -> Result<(), String> {
            let absolute = std::path::absolute(scene).map_err(|error| error.to_string())?;
            let data = serde_json::to_vec(&absolute).map_err(|error| error.to_string())?;
            if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&temporary, data).map_err(|error| error.to_string())?;
            if let Err(error) = std::fs::rename(&temporary, path) {
                let _ = std::fs::remove_file(&temporary);
                return Err(error.to_string());
            }
            Ok(())
        };
        if let Err(error) = write() { log::warn!("Cannot remember scene {}: {error}", scene.display()); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_restoration_preserves_an_earlier_tuning_error() {
        let directory = std::env::temp_dir().join(format!("arpg-startup-errors-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let session = directory.join("last-scene.json");
        let mut app = App { scene_session_path: Some(session.clone()), ..Default::default() };
        let tuning_error = "Invalid tuning file: player speed is negative";
        app.companion_result(&Err(tuning_error.into()));
        app.restore_scene_file();
        assert!(app.companion_error);
        assert_eq!(app.companion_message, tuning_error, "scene selection guidance must not hide a failed tuning load");
        std::fs::write(&session, "invalid JSON").unwrap();
        app.restore_scene_file();
        assert!(app.companion_error);
        assert!(app.companion_message.contains(tuning_error));
        assert!(app.companion_message.contains("Invalid scene preference"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn launches_are_empty_until_a_successfully_played_file_is_selected() {
        let directory = std::env::temp_dir().join(format!("arpg-scene-session-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let session = directory.join("last-scene.json");
        let scene = directory.join("scene.ron");
        let mut app = App { scene_session_path: Some(session.clone()), ..Default::default() };
        app.restore_scene_file();
        assert_eq!(app.game.enemy_count(), 0);
        assert_eq!(app.game.selected_scene_name(), None);
        assert!(!session.exists());
        std::fs::write(&scene, r#"(engine: (name: "chosen", bodies: [(pos: (4.0, 0.0))]))"#).unwrap();
        app.start_scene_file(&scene).unwrap();
        let saved = std::fs::read(&session).unwrap();
        assert_eq!(serde_json::from_slice::<PathBuf>(&saved).unwrap(), scene);
        assert!(app.start_scene_file(&directory.join("missing.ron")).is_err());
        assert_eq!(std::fs::read(&session).unwrap(), saved);
        let mut restored = App { scene_session_path: Some(session.clone()), ..Default::default() };
        restored.restore_scene_file();
        assert_eq!(restored.game.hash(), app.game.hash());
        assert_eq!(restored.game.selected_scene_name(), Some("chosen"));
        std::fs::remove_file(&scene).unwrap();
        let mut missing = App { scene_session_path: Some(session.clone()), ..Default::default() };
        missing.restore_scene_file();
        assert_eq!(missing.game.enemy_count(), 0);
        assert_eq!(missing.game.selected_scene_name(), None);
        assert!(missing.companion_error);
        std::fs::write(&session, "invalid JSON").unwrap();
        let mut corrupt = App { scene_session_path: Some(session), ..Default::default() };
        corrupt.restore_scene_file();
        assert_eq!(corrupt.game.selected_scene_name(), None);
        assert!(corrupt.companion_error);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
