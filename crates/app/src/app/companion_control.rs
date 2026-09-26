//! Native tooling commands and compact observations at the between-tick boundary.
use super::{App, Scene, scene_catalog};
use crate::companion::{Command, Control, ZoomDirection};
use arpg_sim::TuningField;
use serde_json::json;

impl App {
    pub(super) fn companion_result(&mut self, result: &Result<String, String>) {
        self.companion_error = result.is_err();
        self.companion_message = match result { Ok(message) | Err(message) => message.clone() };
    }

    fn apply_control(&mut self, control: Control) -> Result<(String, Option<serde_json::Value>), String> {
        if self.trial.is_some() { return Err("Companion controls are unavailable during a performance trial.".into()); }
        let message = match control {
            Control::SceneEditor(request) => return self.scene_editor_request(*request),
            Control::Tuning(edit) => self.edit_tuning(edit),
            Control::RefreshScenes => {
                self.scene_catalog = Some(scene_catalog(std::path::Path::new("scenes")));
                self.scene_catalog.as_ref().expect("just refreshed").as_ref().map_err(Clone::clone)?;
                Ok("Scene list refreshed.".into())
            }
            Control::StartScene(path) => {
                self.start_scene_file(&path)?;
                Ok(format!("Started {}", path.display()))
            }
            Control::DefaultScene => {
                self.start_playtest(Scene::boot())?;
                Ok("Started default horde.".into())
            }
            Control::Collision(enabled) => {
                self.collision_debug.set_enabled(enabled, &self.game);
                Ok(format!("Collision outlines {}", if enabled { "on" } else { "off" }))
            }
            Control::Vsync(enabled) => self.set_vsync(enabled),
            Control::Zoom(direction) => {
                let camera = self.camera.as_mut().ok_or("No camera yet")?;
                const STEP: f32 = 1.2;
                camera.zoom_by(match direction { ZoomDirection::In => 1.0 / STEP, ZoomDirection::Out => STEP });
                self.perf.reset();
                Ok(format!("Camera height: {:.1} m", camera.view_height()))
            }
            Control::Capture(path) => {
                if self.renderer.is_none() { return Err("No renderer yet".into()); }
                self.capture.request(path, None, std::time::Instant::now())?;
                Ok("Screenshot requested. See Debug for the write result.".into())
            }
        }?;
        Ok((message, None))
    }

    pub(super) fn set_vsync(&mut self, enabled: bool) -> Result<String, String> {
        let renderer = self.renderer.as_mut().ok_or("no renderer yet")?;
        if renderer.vsync() != enabled {
            renderer.toggle_vsync();
            self.perf.reset();
        }
        Ok(format!("vsync {}", renderer.vsync()))
    }

    pub(super) fn companion_reply(&mut self, command: Command) -> String {
        if self.scene_catalog.is_none() {
            self.scene_catalog = Some(scene_catalog(std::path::Path::new("scenes")));
        }
        let result = command.control().and_then(|control| control.map(|control| self.apply_control(control)).transpose());
        match &result {
            Ok(Some((message, _))) => self.companion_result(&Ok(message.clone())),
            Err(error) => self.companion_result(&Err(error.clone())),
            Ok(None) => {}
        }
        let (editor, error) = match result {
            Ok(control) => (control.and_then(|(_, editor)| editor), None),
            Err(error) => (None, Some(error)),
        };
        let catalog = self.scene_catalog.as_ref().expect("initialized catalog");
        let files: Vec<_> = catalog.as_ref().map(|paths| paths.iter().map(|path| {
            json!({"path":path, "label":path.file_name().unwrap_or_default().to_string_lossy()})
        }).collect()).unwrap_or_default();
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
        json!({"ok":error.is_none(), "error":error, "editor":editor, "state":{
            "session":std::process::id(), "fields":fields, "unsaved":tuning != self.tuning_saved,
            "path":self.tuning_path, "message":self.companion_message, "error":self.companion_error,
            "busy":self.trial.is_some(),
            "scenes":{"directory":std::path::absolute("scenes").ok(),"files":files, "error":catalog.as_ref().err(),
                "current":self.game.selected_scene_name(), "run":self.run_id, "tick":self.game.tick()},
            "debug":{"collision":self.collision_debug.enabled(),
                "vsync":self.renderer.as_ref().map(arpg_gfx::Renderer::vsync),
                "view_height":self.camera.as_ref().map(arpg_gfx::OrthoCamera::view_height),
                "fps":self.clock.fps(), "frame_ms":self.clock.frame_ms(), "enemies":self.game.enemy_count(),
                "instances":self.render_instance_count(),
                "capture":self.capture.report()}
        }}).to_string()
    }

    pub(super) fn drain_companion(&mut self) {
        // Server permits one outstanding request; no socket I/O occurs on this thread.
        let request = self.companion_server.as_ref().and_then(|server| server.requests.try_recv().ok());
        if let Some(request) = request {
            let response = if std::time::Instant::now() >= request.expires {
                json!({"ok":false,"error":"Request expired without being applied"}).to_string()
            } else { self.companion_reply(request.command) };
            let _ = request.reply.send(response);
        }
    }

    pub(super) fn open_companion(&self) -> Result<(), String> {
        let server = self.companion_server.as_ref().ok_or("Companion socket is unavailable; check the game log")?;
        let bundle = std::path::absolute("target/ARPG Companion.app").map_err(|e| e.to_string())?;
        if !bundle.is_dir() { return Err("Build the native companion with: python3 scripts/companion.py build".into()); }
        let status = std::process::Command::new("open").arg(&bundle).arg("--args").arg("--socket").arg(&server.path)
            .status().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err("macOS could not open ARPG Companion".into()) }
    }
}

impl App {
    fn scene_editor_request(&mut self, request: crate::scene_editor::Request) -> Result<(String, Option<serde_json::Value>), String> {
        use crate::scene_editor::{Request, document};
        let (message, draft) = match request {
            Request::New => {
                let scene = Scene { engine: arpg_sim::Scene { name: "Untitled encounter".into(), ..Default::default() }, ..Default::default() };
                ("New scene draft. The current game is unchanged.".into(), document(&scene, None, None, None)?)
            }
            Request::Open { path } => {
                let path = std::path::absolute(path).map_err(|e| e.to_string())?;
                let (scene, revision) = arpg_content::read_scene_document(&path)?;
                ("Scene opened for editing.".into(), document(&scene, None, Some(path), Some(revision))?)
            }
            Request::Edit { scene, edit } => {
                let (scene, selected) = crate::scene_editor::edit(scene, edit)?;
                ("Draft updated. Save & play to try it.".into(), document(&scene, selected, None, None)?)
            }
            Request::Save { scene, path, revision, play } => {
                let path = std::path::absolute(path).map_err(|e| e.to_string())?;
                let mut draft = document(&scene, None, Some(path.clone()), None)?;
                // Prepare the exact playable replacement before touching the file.
                // Commit and adapter cleanup after persistence are infallible.
                let prepared = if play {
                    let run = self.run_id.checked_add(1).ok_or("playtest identities exhausted")?;
                    let game = self.game.prepare_scene(&scene).map_err(|e| e.to_string())?;
                    Some((run, game))
                } else { None };
                let revision = arpg_content::save_scene_document(&path, &scene, revision.as_deref())?;
                if let Some((run, game)) = prepared { self.game = game; self.finish_playtest_start(run); }
                self.scene_catalog = Some(scene_catalog(std::path::Path::new("scenes")));
                draft["revision"] = json!(revision);
                (if play { "Scene saved and started." } else { "Scene saved." }.into(), draft)
            }
        };
        Ok((message, Some(draft)))
    }
}
