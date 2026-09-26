//! Runtime tuning admission and persistence shared by companion and harness.
use super::App;
use crate::companion::Edit;

impl App {
    pub(super) fn load_saved_tuning(&mut self) {
        self.tuning_path = std::env::var_os("ARPG_TUNING_FILE").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "tuning.ron".into());
        let result = match arpg_content::load_tuning(&self.tuning_path) {
            Ok(tuning) => {
                self.game.set_tuning(tuning);
                self.tuning_saved = tuning;
                Ok(format!("Loaded {}", self.tuning_path.display()))
            }
            Err(arpg_content::LoadError::Read(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok("Built-in settings".into())
            }
            Err(error) => Err(error.to_string()),
        };
        self.companion_result(&result);
    }

    /// All companion and harness edits land here, between simulation ticks.
    pub(super) fn edit_tuning(&mut self, edit: Edit) -> Result<String, String> {
        if self.trial.is_some() { return Err("Tuning is unavailable during a performance trial.".into()); }
        match edit {
            Edit::Set(field, value) => {
                let next = self.game.tuning().with(field, value)?;
                self.game.set_tuning(next);
                self.perf.reset();
                Ok(format!("{} = {} {}", field.attribute().label, value, field.attribute().unit))
            }
            Edit::Save => {
                arpg_content::save_tuning(&self.tuning_path, self.game.tuning())?;
                self.tuning_saved = self.game.tuning();
                Ok(format!("Saved {}", self.tuning_path.display()))
            }
            Edit::Revert | Edit::Defaults => {
                let tuning = if matches!(edit, Edit::Revert) { self.tuning_saved } else { Default::default() };
                self.game.set_tuning(tuning);
                self.perf.reset();
                Ok("Settings restored. Restart the encounter to compare.".into())
            }
            Edit::Restart => {
                self.restart_playtest()?;
                Ok("Encounter restarted with current tuning.".into())
            }
        }
    }
}
