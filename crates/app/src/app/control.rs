//! Harness dispatch and observations at the app's between-frame boundary.

use super::{App, Deferred, read_scene};
use crate::harness::{Command, Request};
use crate::presentation::{default_character_path, default_horde_path};
use crate::presentation::{report_character_preview, report_horde_preview};
use arpg_core::Report;
use arpg_gfx::{OrthoCamera, Renderer};
use glam::Vec2;

impl App {
    pub(super) fn render_instance_count(&self) -> u64 {
        u64::from(self.character_preview.is_some())
            + self.horde_preview.as_ref().map_or(0, |horde| horde.instance_count() as u64)
            + self.world_assets.as_ref().map_or(0, |world| world.instance_count() as u64)
            + self.collision_debug.drawings().len() as u64
    }

    /// Applies whatever the control socket has sent since the last frame.
    ///
    /// Runs between redraws. Injected action edges wait for the next simulation tick.
    pub(super) fn drain_harness(&mut self) -> bool {
        // Collected up front so the receiver borrow ends before the loop needs
        // the rest of `self`.
        let Some(rx) = &self.harness else {
            return false;
        };
        let requests: Vec<Request> = rx.try_iter().collect();
        let mut quit = false;

        for Request { command, reply } in requests {
            if self.trial.is_some() && !matches!(command, Command::Quit) {
                let _ = reply.send("error: performance trial already running".into());
                continue;
            }
            let now = std::time::Instant::now();
            let answer = match command {
                Command::TuningEdit(edit) => {
                    let result = self.edit_tuning(edit);
                    self.companion_result(&result);
                    result.unwrap_or_else(|error| format!("error: {error}"))
                }
                Command::ShowCharacter(path) => match self.show_character(path) {
                    Ok(answer) => answer,
                    Err(error) => format!("error: {error}"),
                },
                Command::ClearCharacter => match self.show_character(default_character_path()) {
                    Ok(_) => "ok".to_string(),
                    Err(error) => format!("error: {error}"),
                },
                Command::ShowHorde(path) => match self.show_horde(path) {
                    Ok(answer) => answer,
                    Err(error) => format!("error: {error}"),
                },
                Command::ClearHorde => match self.show_horde(default_horde_path()) {
                    Ok(_) => "ok".to_string(),
                    Err(error) => format!("error: {error}"),
                },
                Command::StartScene(path) => match self.start_scene_file(&path) {
                    Ok(()) => self.ready_reply(),
                    Err(error) => format!("error: {error}"),
                },
                Command::RestartScene => match self.restart_playtest() {
                    Ok(()) => self.ready_reply(),
                    Err(error) => format!("error: {error}"),
                },
                Command::AddScene(path) => match read_scene(&path).and_then(|scene| {
                    self.game
                        .load_scene(&scene)
                        .map_err(|error| error.to_string())
                }) {
                    Ok(id) => {
                        self.audio.sync_loops(&self.game);
                        self.perf.reset();
                        format!(
                            "ready run={} scene={id} tick={}",
                            self.run_id,
                            self.game.tick()
                        )
                    }
                    Err(error) => format!("error: {error}"),
                },
                Command::EvictScene(id) => {
                    if self.game.evict_scene(id) {
                        self.audio.sync_loops(&self.game);
                        self.perf.reset();
                        format!("evicted run={} scene={id}", self.run_id)
                    } else {
                        format!("error: no live scene {id} in run {}", self.run_id)
                    }
                }
                Command::ListScenes => {
                    let mut out = Report::default();
                    out.int("run_id", self.run_id);
                    for (id, name) in self.game.scene_instances() {
                        out.text(&id.to_string(), name);
                    }
                    out.finish()
                }
                Command::Cursor(pixel) => match self.input.on_cursor(pixel) {
                    Ok(()) => "ok".into(),
                    Err(error) => error.into(),
                },
                Command::Press(key) => {
                    self.on_key(key, true, false);
                    "ok".to_string()
                }
                Command::Release(key) => {
                    self.on_key(key, false, false);
                    "ok".to_string()
                }
                Command::Tap(key) => {
                    self.on_key(key, true, false);
                    self.scheduled.push(Deferred {
                        due: now,
                        release: Some(key),
                        reply: None,
                    });
                    "ok".to_string()
                }
                Command::Hold(key, ms) => {
                    self.defer(Some(key), ms, reply);
                    continue;
                }
                Command::Wait(ms) => {
                    self.defer(None, ms, reply);
                    continue;
                }
                Command::Shot(path) => {
                    if self.renderer.is_none() {
                        "error: no renderer yet".to_string()
                    } else {
                        let _ = self.capture.request(path, Some(reply), now);
                        continue; // Capture owns the reply through write, timeout or cancellation.
                    }
                }
                Command::State => self.report_state(),
                Command::PerfTrial(spec) => {
                    if self.game.tuning() != arpg_sim::Tuning::default() {
                        "error: performance trials require default tuning".into()
                    } else if self.renderer.is_none() {
                        "error: no renderer yet".into()
                    } else if let Err(error) = self.performance_readiness() {
                        format!("error: {error}")
                    } else if let Err(error) = self.restart_playtest() {
                        format!("error: {error}")
                    } else {
                        self.trial = Some(crate::trial::Trial::new(spec, reply, self.game.enemy_count()));
                        continue;
                    }
                }
                Command::Perf { samples } => self.report_perf(samples),
                Command::ResetPerf => {
                    self.perf.reset();
                    "ok".to_string()
                }
                Command::PreparePerf => {
                    if let Some(window) = &self.window {
                        window.focus_window();
                        "ok".into()
                    } else {
                        "error: no window yet".into()
                    }
                }
                Command::Impulse { target, value } => match self.game.body_named(&target) {
                    Some(id) if self.game.apply_impulse(id, value) => {
                        format!("impulse applied to {id}")
                    }
                    _ => format!("error: no physical body {target}"),
                },
                Command::TraceSince(tick) => self.report_trace(tick),
                Command::Spawn { x, z, what } => {
                    // The reply says *queued*, not spawned, because that is what
                    // happened: the body appears when the next tick runs. A
                    // reply claiming otherwise would make a `state` taken
                    // immediately afterwards look like a bug.
                    if let Err(error) = what.validate() {
                        format!("error: {error}")
                    } else if self.game.request_spawn(Vec2::new(x, z), what).is_some() {
                        format!("queued at ({x}, {z})")
                    } else {
                        "error: invalid placement or spawn queue full".to_string()
                    }
                }
                Command::Source(spec) => {
                    match spec.try_into() {
                        Ok(source) => format!("source {}", self.game.add_source(source)),
                        Err(error) => format!("error: {error}"),
                    }
                }
                Command::SetSourceEnabled { id, enabled } => {
                    if self.game.set_source_enabled(id, enabled) {
                        format!("source {id} enabled={enabled}")
                    } else {
                        format!("error: no live source {id}")
                    }
                }
                Command::RemoveSource(id) => match self.game.remove_source(id) {
                    true => format!("removed {id}"),
                    false => format!("error: no live source {id}"),
                },
                Command::SetCollisionDebug(on) => {
                    self.collision_debug.set_enabled(on, &self.game);
                    format!("debug collision {}", if on { "on" } else { "off" })
                }
                Command::SetVsync(on) => self.set_vsync(on).unwrap_or_else(|error| format!("error: {error}")),
                Command::SetAudioEnabled(on) => {
                    self.audio.set_muted(!on);
                    format!("audio {}", if on { "on" } else { "off" })
                }
                Command::Quit => {
                    if let Some(trial) = self.trial.take() { trial.finish("error: cancelled by quit".into()); }
                    quit = true;
                    "ok".to_string()
                }
            };
            let _ = reply.send(answer);
        }
        quit
    }

    pub(super) fn performance_readiness(&self) -> Result<(), &'static str> {
        let window = self.window.as_ref().ok_or("no window yet")?;
        if !window.has_focus() { return Err("performance window is not focused"); }
        if self.occluded { return Err("performance window is occluded"); }
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 { return Err("performance surface has zero size"); }
        Ok(())
    }

    /// Compact observations: never invoke Game::report or Game::hash here.
    pub(super) fn report_perf(&self, samples: bool) -> String {
        let mut out = Report::default();
        self.write_perf(&mut out, samples);
        out.finish()
    }

    pub(super) fn write_perf(&self, out: &mut Report, samples: bool) {
        out.int("schema_version", 1);
        out.bool("debug_build", cfg!(debug_assertions));
        out.int("run_id", self.run_id);
        out.int("current_tick", self.game.tick());
        out.int("current_enemies", self.game.enemy_count() as u64);
        out.int("current_corpses", self.game.corpses().len() as u64);
        out.bool("vsync", self.renderer.as_ref().is_some_and(Renderer::vsync));
        out.text("selected_scene", self.game.selected_scene_name().unwrap_or(""));
        out.bool("collision_debug", self.collision_debug.enabled());
        out.object("readiness", |out| {
            out.bool("focused", self.window.as_ref().is_some_and(|window| window.has_focus()));
            out.bool("occluded", self.occluded);
            out.bool("surface", self.renderer.is_some() && self.window.as_ref().is_some_and(|window| {
                let size = window.inner_size();
                size.width > 0 && size.height > 0
            }));
        });
        if let Some(camera) = &self.camera { out.num("view_height", camera.view_height()); }
        if let Some(window) = &self.window {
            let size = window.inner_size();
            out.int("width", u64::from(size.width));
            out.int("height", u64::from(size.height));
            out.num("scale_factor", window.scale_factor() as f32);
            out.int("refresh_millihertz", window.current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz()).map_or(0, u64::from));
        }
        if let Some(renderer) = &self.renderer {
            out.object("gpu", |out| renderer.report_performance_context(out));
        }
        out.object("horde", |out| report_horde_preview(self.horde_preview.as_ref(), out));
        out.object("character", |out| report_character_preview(self.character_preview.as_ref(), out));
        self.perf.report(out, samples);
    }

    /// Game state and app presentation diagnostics as JSON.
    pub(super) fn report_state(&self) -> String {
        let mut out = Report::default();

        out.int("run_id", self.run_id);
        out.text(
            "selected_scene",
            self.game.selected_scene_name().unwrap_or(""),
        );
        out.text("sim_hash", &format!("{:016x}", self.game.hash()));
        out.object("sim", |sim| self.game.report(sim));
        out.object("tuning", |out| {
            out.bool("unsaved", self.game.tuning() != self.tuning_saved);
            out.text("path", &self.tuning_path.display().to_string());
        });
        out.object("companion", |out| {
            out.text("message", &self.companion_message);
            out.bool("error", self.companion_error);
        });
        out.object("world_assets", |out| {
            if let Some(world) = &self.world_assets {
                world.report(out);
            }
        });
        out.object("character_preview", |character| {
            report_character_preview(self.character_preview.as_ref(), character);
        });
        out.object("horde_preview", |horde| {
            report_horde_preview(self.horde_preview.as_ref(), horde);
        });

        out.object("attack_effects", |out| self.attack_effects.report(out));
        out.object("audio", |out| self.audio.report(out));
        out.object("collision_debug", |out| self.collision_debug.report(out));
        out.object("render", |r| {
            let target = self
                .camera
                .as_ref()
                .map(OrthoCamera::target)
                .unwrap_or_default();
            r.vec3("camera_target", target);
            let player_instances = u64::from(self.character_preview.is_some());
            let horde_instances = self
                .horde_preview
                .as_ref()
                .map_or(0, |horde| horde.instance_count() as u64);
            let static_instances = self
                .world_assets
                .as_ref()
                .map_or(0, |world| world.instance_count() as u64);
            r.int("instances", self.render_instance_count());
            let effect_vertices = self.world_assets.as_ref()
                .map_or(0, |world| world.torches.effects().len());
            r.int("effect_vertices", effect_vertices as u64);
            r.int("effect_draws", u64::from(effect_vertices > 0));
            r.int("weather_vertices", self.atmosphere.vertices().len() as u64);
            r.int("weather_draws", u64::from(!self.atmosphere.vertices().is_empty()));
            r.int(
                "debug_outline_instances",
                self.collision_debug.drawings().len() as u64,
            );
            r.int(
                "debug_draws",
                u64::from(!self.collision_debug.drawings().is_empty()),
            );
            r.int("static_mesh_instances", static_instances);
            r.int(
                "static_mesh_draws",
                self.world_assets
                    .as_ref()
                    .map_or(0, |world| world.draw_count() as u64),
            );
            r.int("player_character_instances", player_instances);
            r.int("player_character_draws", self.character_preview.as_ref().map_or(0, |c| c.draw_count() as u64));
            r.int("horde_character_instances", horde_instances);
            r.int(
                "horde_character_draws",
                self.horde_preview
                    .as_ref()
                    .map_or(0, |horde| horde.draw_count() as u64),
            );
            r.int("frames", self.frames);
            r.int("skipped", self.skipped);
            r.num("frame_ms", self.clock.frame_ms());
            r.bool("vsync", self.renderer.as_ref().is_some_and(Renderer::vsync));
        });

        out.finish()
    }

    /// Every trace event from `tick` onward, one per line.
    ///
    /// Same rendering as a scenario's golden trace file, because it is the same
    /// function — a second formatter would be a second thing to keep in step,
    /// and the whole point is that what you read here is what a scenario can
    /// assert on.
    fn report_trace(&self, tick: u64) -> String {
        let mut out = format!("# run {}\n", self.run_id);
        let dropped = self.game.trace_dropped();
        if dropped > 0 {
            out.push_str(&format!(
                "# {dropped} event(s) dropped; the buffer wrapped\n"
            ));
        }
        let events = self.game.render_trace_since(tick);
        out.push_str(&events);
        out.push_str(&format!(
            "# {} event(s)",
            self.game.trace().since(tick).count() + self.game.encounter_trace().since(tick).count()
        ));
        out
    }

    /// Validate a deadline before pressing a key, so rejection cannot leave it held.
    fn defer(
        &mut self,
        key: Option<winit::keyboard::KeyCode>,
        ms: u64,
        reply: std::sync::mpsc::Sender<String>,
    ) {
        let Some(due) = std::time::Instant::now().checked_add(std::time::Duration::from_millis(ms))
        else {
            let _ = reply.send("error: duration exceeds the clock's range".into());
            return;
        };
        if let Some(key) = key {
            self.on_key(key, true, false);
        }
        self.scheduled.push(Deferred {
            due,
            release: key,
            reply: Some(reply),
        });
    }

    /// Fires the releases and replies whose deadline has passed.
    pub(super) fn service_schedule(&mut self) {
        let now = std::time::Instant::now();
        self.scheduled.retain(|deferred| {
            if now < deferred.due {
                return true;
            }
            if let Some(key) = deferred.release {
                self.input
                    .on_key(key, false, false);
            }
            if let Some(reply) = &deferred.reply {
                let _ = reply.send("ok".to_string());
            }
            false
        });
    }
}
