//! Tuning admission and companion commands at the between-tick app boundary.
use super::App;
use crate::tuning::{Command, Edit};
use arpg_sim::TuningField;
use serde_json::json;

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
        self.tuning_result(&result);
    }

    fn tuning_result(&mut self, result: &Result<String, String>) {
        self.tuning_error = result.is_err();
        self.tuning_message = match result { Ok(message) | Err(message) => message.clone() };
    }

    /// All companion and harness edits land here, between simulation ticks.
    pub(super) fn edit_tuning(&mut self, edit: Edit) -> Result<String, String> {
        let result = (|| {
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
        })();
        self.tuning_result(&result);
        result
    }

    pub(super) fn tuning_reply(&mut self, command: Command) -> String {
        let result = command.edit().and_then(|edit| edit.map(|edit| self.edit_tuning(edit)).transpose());
        if let Err(error) = &result { self.tuning_result(&Err(error.clone())); }
        let tuning = self.game.tuning();
        let fields: Vec<_> = TuningField::ALL.into_iter().map(|field| {
            let a = field.attribute();
            json!({"key":a.key, "group":a.group, "label":a.label, "unit":a.unit,
                "default":a.default, "min":a.min, "max":a.max, "step":a.step,
                "value":tuning.get(field), "saved":self.tuning_saved.get(field),
                "note":(field == TuningField::MonsterAcceleration).then(|| format!(
                    "Motor speed ceiling: {:.2} m/s. Increase acceleration to reach higher target speeds.",
                    tuning.monster_speed_limit()))})
        }).collect();
        json!({"ok":result.is_ok(), "error":result.err(), "state":{
            "session":std::process::id(), "fields":fields, "unsaved":tuning != self.tuning_saved,
            "path":self.tuning_path, "message":self.tuning_message, "error":self.tuning_error,
            "busy":self.trial.is_some()
        }}).to_string()
    }

    pub(super) fn drain_tuning(&mut self) {
        // Server permits one outstanding request; no socket I/O occurs on this thread.
        let request = self.tuning_server.as_ref().and_then(|server| server.requests.try_recv().ok());
        if let Some(request) = request {
            let response = if std::time::Instant::now() >= request.expires {
                json!({"ok":false,"error":"Request expired without being applied"}).to_string()
            } else { self.tuning_reply(request.command) };
            let _ = request.reply.send(response);
        }
    }

    pub(super) fn open_tuning_companion(&self) -> Result<(), String> {
        let server = self.tuning_server.as_ref().ok_or("Tuning socket is unavailable; check the game log")?;
        let bundle = std::path::absolute("target/ARPG Tuning.app").map_err(|e| e.to_string())?;
        if !bundle.is_dir() { return Err("Build the native panel with: python3 scripts/tuning.py build".into()); }
        let status = std::process::Command::new("open").arg(&bundle).arg("--args").arg("--socket").arg(&server.path)
            .status().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err("macOS could not open ARPG Tuning".into()) }
    }
}
