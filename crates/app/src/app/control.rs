//! Harness dispatch and observations at the app's between-frame boundary.

use super::{App, Deferred, read_scene};
use crate::harness::{Command, Request};
use crate::presentation::{default_character_path, default_horde_path};
use crate::presentation::{report_asset_preview, report_character_preview, report_horde_preview};
use arpg_core::Report;
use arpg_gfx::{OrthoCamera, Renderer};
use glam::Vec2;

impl App {
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
            let now = std::time::Instant::now();
            let answer = match command {
                Command::ShowAsset(path) => match self.show_asset(path) {
                    Ok(answer) => answer,
                    Err(error) => format!("error: {error}"),
                },
                Command::ClearAsset => {
                    self.asset_preview = None;
                    "ok".to_string()
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
                Command::Press(key) => {
                    self.handle_key(key, true, false);
                    "ok".to_string()
                }
                Command::Release(key) => {
                    self.handle_key(key, false, false);
                    "ok".to_string()
                }
                Command::Tap(key) => {
                    self.handle_key(key, true, false);
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
                        self.capture.request(path, Some(reply), now);
                        continue; // Capture owns the reply through write, timeout or cancellation.
                    }
                }
                Command::State => self.report_state(),
                Command::Impulse { target, value } => match self.game.body_named(&target) {
                    Some(id) if self.game.apply_impulse(id, value) => {
                        format!("impulse applied to {id}")
                    }
                    _ => format!("error: no physical body {target}"),
                },
                Command::TraceSince(tick) => self.report_trace(tick),
                Command::SetEnemies(n) => {
                    self.game.set_enemy_count(n);
                    // Respawning retires enemy identities and their behaviours;
                    // props and the player survive. Said in the reply rather than left
                    // to be discovered, because "I set the horde and the
                    // chasing stopped" is otherwise a puzzle.
                    format!(
                        "enemies {} seekers {}",
                        self.game.enemy_count(),
                        self.game.seeker_count()
                    )
                }
                Command::Spawn { x, z, what } => {
                    // The reply says *queued*, not spawned, because that is what
                    // happened: the body appears when the next tick runs. A
                    // reply claiming otherwise would make a `state` taken
                    // immediately afterwards look like a bug.
                    if self.game.request_spawn(Vec2::new(x, z), what) {
                        format!("queued at ({x}, {z}) seeks={}", what.seeks())
                    } else {
                        "error: spawn queue full".to_string()
                    }
                }
                Command::Source(spec) => {
                    // The conversion is where `Placement::around` and the
                    // cadence clamp are applied, so the socket cannot reach a
                    // source that skipped either.
                    let id = self.game.add_source(spec.into());
                    format!("source {id}")
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
                Command::SetSeekers(n) => {
                    self.game.set_seeker_count(n);
                    format!("seekers {}", self.game.seeker_count())
                }
                Command::SetCollisionDebug(on) => {
                    self.collision_debug.set_enabled(on, &self.game);
                    format!("debug collision {}", if on { "on" } else { "off" })
                }
                Command::SetVsync(on) => match self.renderer.as_mut() {
                    Some(renderer) => {
                        if renderer.vsync() != on {
                            renderer.toggle_vsync();
                        }
                        format!("vsync {}", renderer.vsync())
                    }
                    None => "error: no renderer yet".to_string(),
                },
                Command::Quit => {
                    quit = true;
                    "ok".to_string()
                }
            };
            let _ = reply.send(answer);
        }
        quit
    }

    /// Everything worth knowing about the running program, as JSON.
    ///
    /// **Derived, not hand-written.** The simulation's half comes from
    /// `Game::report`, which destructures `Game` exhaustively — so a field
    /// added to the game requires an explicit reporting decision. What is left
    /// here is the part `sim` genuinely cannot know: how the frame went, what
    /// the camera is doing, whether the renderer is presenting.
    ///
    /// JSON rather than a positional line, because the consumer is usually a
    /// program: `jq -r .sim.tick` does not care what order the fields are in or
    /// how many were added since it was written.
    pub(super) fn report_state(&self) -> String {
        let mut out = Report::default();

        out.int("run_id", self.run_id);
        out.text(
            "selected_scene",
            self.game.selected_scene_name().unwrap_or(""),
        );
        out.text("sim_hash", &format!("{:016x}", self.game.hash()));
        out.object("sim", |sim| self.game.report(sim));
        out.object("ui", |ui| self.input.menu().report(ui));
        out.object("asset_preview", |asset| {
            report_asset_preview(self.asset_preview.as_ref(), asset);
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
                .map_or(0, |world| world.instance_count() as u64)
                + u64::from(self.asset_preview.is_some());
            r.int(
                "instances",
                player_instances
                    + horde_instances
                    + static_instances
                    + self.collision_debug.drawings().len() as u64,
            );
            r.int(
                "effect_vertices",
                self.attack_effects.vertices().len() as u64,
            );
            r.int(
                "effect_draws",
                u64::from(!self.attack_effects.vertices().is_empty()),
            );
            r.int(
                "debug_disc_instances",
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
                    .map_or(0, |world| world.draw_count() as u64)
                    + u64::from(self.asset_preview.is_some()),
            );
            r.int("player_character_instances", player_instances);
            r.int("horde_character_instances", horde_instances);
            r.int(
                "horde_pose_draws",
                self.horde_preview
                    .as_ref()
                    .map_or(0, |horde| horde.occupied_buckets() as u64),
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
            self.game.trace().since(tick).count() + self.game.control_trace().since(tick).count()
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
            self.handle_key(key, true, false);
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
                    .on_key(key, false, false, self.game.attack_status().profile);
            }
            if let Some(reply) = &deferred.reply {
                let _ = reply.send("ok".to_string());
            }
            false
        });
    }
}
