//! Native lifecycle, playtest replacement and the simulation-to-render frame loop.

mod capture;
mod control;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use glam::Vec2;

use arpg_core::{Action, Intent, MoveDir};
use arpg_game::{Game, GameScene as Scene};
use arpg_gfx::{CharacterMesh, FrameOutcome, MeshAsset, OrthoCamera, QuadBuffer, Renderer};
use arpg_sim::{Accumulator, Alpha};

use crate::harness::{self, Request};
use crate::hud;
use crate::input::Controls;
use crate::presentation::{
    AssetPreview, LoadedCharacter, LoadedHorde, WorldAssets, default_character_path,
    default_horde_path, presentation_seconds, preview_instance, replace_character,
    replace_horde, replace_preview,
};
use crate::time::Clock;
use crate::ui::{MenuRequest, SceneChoice};

/// Owns the native app and connects gameplay to presentation.
/// `gfx` and `game` have no dependency on each other.
#[derive(Default)]
pub(crate) struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// App-global presentation selected independently of playable scene state.
    asset_preview: Option<AssetPreview<MeshAsset>>,
    /// Ground and prop resources exist together once GPU initialization succeeds.
    world_assets: Option<WorldAssets<MeshAsset>>,
    /// Hierarchical character presentation, also independent of simulation.
    character_preview: Option<LoadedCharacter<CharacterMesh>>,
    /// Shared-pose horde presentation, independent of playable scene state.
    horde_preview: Option<LoadedHorde<CharacterMesh>>,
    camera: Option<OrthoCamera>,
    game: Game,
    collision_debug: crate::collision_debug::CollisionDebug,
    attack_effects: crate::attack_effects::AttackEffects,
    /// Game ids and ticks are scoped to this playtest generation.
    run_id: u64,
    /// Numbers keyboard screenshots within this process; paths may exist from an earlier run.
    captures: u32,
    /// Frames skipped because the surface had none to give — occluded,
    /// resized behind our back, or lost. Reported alongside `frames` so a
    /// suspiciously fast run is self-diagnosing rather than mysterious.
    skipped: u64,
    /// Frames presented since launch.
    ///
    /// Reported by the harness so throughput can be *counted* over a known
    /// interval rather than inferred from `Clock`'s smoothed average — which is
    /// an EMA, and so cannot distinguish a steady 60Hz from a mixture that
    /// averages to it. Compare counts over wall time to detect background throttling.
    frames: u64,
    /// Present only when `ARPG_HARNESS` asked for a control socket.
    harness: Option<std::sync::mpsc::Receiver<Request>>,
    /// Keys to release, and replies to send, once their deadline passes.
    scheduled: Vec<Deferred>,
    capture: capture::Capture,
    /// Reused overlay staging buffer; reset through its bounded sink each frame.
    quads: QuadBuffer,
    input: Controls,
    clock: Clock,
    /// Turns the frame's elapsed seconds into whole simulation ticks.
    ///
    /// Owned by `app` because the frame loop is here, but defined in `sim`,
    /// which alone mints `Dt`. The app supplies elapsed seconds to the
    /// accumulator; `Game::step` accepts only the resulting fixed-duration ticks.
    accumulator: Accumulator,
}

/// A key release, harness reply, or both, owed once `due` passes.
struct Deferred {
    due: std::time::Instant,
    /// The key to lift, if this deadline releases one.
    release: Option<KeyCode>,
    /// The caller to answer, if one is blocked on this deadline.
    reply: Option<std::sync::mpsc::Sender<String>>,
}

/// One notch of zoom per keypress.
const ZOOM_STEP: f32 = 1.2;
const _: () = assert!(ZOOM_STEP > 1.0, "a step of 1 or less makes the zoom keys inert or inverted");

/// Where `P` writes screenshots. The system temp directory unless told
/// otherwise, so a stray keypress never litters the working tree.
fn capture_dir() -> std::path::PathBuf {
    std::env::var_os("ARPG_CAPTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Preserve the file identity in diagnostics from either caller.
fn read_scene(path: &std::path::Path) -> Result<Scene, String> {
    arpg_content::load_scene(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Discovery happens only when the picker opens. Do not parse here: a broken
/// file remains selectable so its load error can be shown without losing a run.
fn scene_catalog(directory: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let read = || -> std::io::Result<Vec<std::path::PathBuf>> {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ron") && entry.file_type()?.is_file() {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    };
    read().map_err(|error| format!("Cannot list {}: {error}", directory.display()))
}

impl App {
    fn show_asset(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let Self { renderer, asset_preview, .. } = self;
        let renderer = renderer.as_ref().ok_or_else(|| "no renderer yet".to_string())?;
        replace_preview(asset_preview, path, |mesh| {
            renderer.upload_mesh(mesh).map_err(|error| error.to_string())
        })?;
        Ok(asset_preview
            .as_ref()
            .expect("successful replacement selects a preview")
            .summary())
    }

    fn show_character(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let Self {
            renderer,
            character_preview,
            ..
        } = self;
        let renderer = renderer
            .as_ref()
            .ok_or_else(|| "no renderer yet".to_string())?;
        replace_character(character_preview, path, |character| {
            renderer
                .upload_character(character)
                .map_err(|error| error.to_string())
        })?;
        Ok(character_preview
            .as_ref()
            .expect("successful replacement selects a character")
            .summary())
    }

    fn show_horde(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let Self {
            renderer,
            horde_preview,
            ..
        } = self;
        let renderer = renderer
            .as_ref()
            .ok_or_else(|| "no renderer yet".to_string())?;
        replace_horde(horde_preview, path, |character| {
            renderer
                .upload_character(character)
                .map_err(|error| error.to_string())
        })?;
        Ok(horde_preview
            .as_ref()
            .expect("successful replacement selects a horde")
            .summary())
    }

    /// Both native keys and harness keys execute menu requests at this same
    /// between-tick boundary. No file I/O or game replacement happens in HUD drawing.
    fn handle_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) -> bool {
        let consumed = self.input.on_key(key, pressed, repeat, self.game.attack_status().profile);
        match self.input.menu_mut().take_request() {
            Some(MenuRequest::RefreshScenes) => {
                self.input.menu_mut().set_catalog(scene_catalog(std::path::Path::new("scenes")));
            }
            Some(MenuRequest::Start(choice)) => {
                let result = match choice {
                    SceneChoice::Restart => self.restart_playtest(),
                    SceneChoice::Boot => self.start_playtest(Scene::boot()),
                    SceneChoice::File { path, .. } => self.start_scene_file(&path),
                };
                if let Err(error) = result {
                    self.input.menu_mut().set_error(error);
                }
            }
            None => {}
        }
        consumed
    }

    fn start_scene_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.start_playtest(read_scene(path)?)
    }

    fn restart_playtest(&mut self) -> Result<(), String> {
        let run_id = self.run_id.checked_add(1).ok_or("playtest identities exhausted")?;
        let old_profile = self.game.attack_status().profile;
        self.game.restart().map_err(|error| error.to_string())?;
        self.finish_playtest_start(run_id, old_profile);
        Ok(())
    }

    /// Prepare before committing: a bad replacement cannot destroy the current
    /// playtest. Construction is synchronous and runs zero simulation ticks.
    fn start_playtest(&mut self, scene: Scene) -> Result<(), String> {
        let run_id = self.run_id.checked_add(1).ok_or("playtest identities exhausted")?;
        let old_profile = self.game.attack_status().profile;
        self.game.start_scene(&scene).map_err(|error| error.to_string())?;
        self.finish_playtest_start(run_id, old_profile);
        Ok(())
    }

    /// Infallible adapter cleanup after Game has installed a complete replacement.
    /// Device input, captures, and camera state belong to the app, not gameplay.
    fn finish_playtest_start(&mut self, run_id: u64, old_profile: arpg_sim::AttackProfile) {
        // Old delayed key releases must not lift a new run's presses. Pending
        // callers receive a cancellation rather than a success for another run.
        for deferred in self.scheduled.drain(..) {
            if let Some(key) = deferred.release {
                self.input.on_key(key, false, false, old_profile);
            }
            if let Some(reply) = deferred.reply {
                let _ = reply.send("error: cancelled by playtest restart".into());
            }
        }
        self.capture.finish(Err("cancelled by playtest restart".into()));
        self.attack_effects.clear();
        self.input.restart();
        self.run_id = run_id;
        self.accumulator = Accumulator::default();
        if let Some(character) = &mut self.character_preview {
            character.reset();
        }
        if let Some(horde) = &mut self.horde_preview {
            horde.clear();
        }
        if let Some(camera) = &mut self.camera {
            camera.snap_to(self.game.player_pos());
        }
        if let Some(world) = &mut self.world_assets {
            world.rebuild(self.game.prop_presentations(Alpha::ZERO));
        }
        self.collision_debug.rebuild(&self.game);
        self.quads.sink();
        // Exclude file reading, construction, and the old run's frame remainder.
        self.clock = Clock::default();
    }

    fn ready_reply(&self) -> String {
        format!("ready run={} tick=0 hash={:016x}", self.run_id, self.game.hash())
    }

    /// One-shot debug shortcuts bypass the tick-sampled gameplay action layer.
    /// Menu handling must get first refusal, especially for Escape. Vsync,
    /// screenshots and exit are handled in `window_event`; these edit the
    /// world or camera. Debug keys must remain disjoint from game bindings.
    fn on_debug_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::F3 => self.collision_debug.set_enabled(!self.collision_debug.enabled(), &self.game),
            // Doubling rather than stepping: the interesting range spans three
            // orders of magnitude, and the knee is easier to find by bisection
            // than by walking. Both directions clamp inside `set_enemy_count`.
            KeyCode::BracketRight => {
                // `max(1)` because doubling zero is zero: the horde can now
                // be emptied, and without this `]` could not refill it.
                self.game.set_enemy_count((self.game.enemy_count() * 2).max(1));
                log::info!("N = {}", self.game.enemy_count());
            }
            KeyCode::BracketLeft => {
                self.game.set_enemy_count(self.game.enemy_count() / 2);
                log::info!("N = {}", self.game.enemy_count());
            }
            KeyCode::Equal => {
                if let Some(c) = &mut self.camera {
                    c.zoom_by(1.0 / ZOOM_STEP);
                }
            }
            KeyCode::Minus => {
                if let Some(c) = &mut self.camera {
                    c.zoom_by(ZOOM_STEP);
                }
            }
            _ => {}
        }
    }

    /// One frame: measure it, run whatever ticks it bought, then draw.
    ///
    /// Everything below the `alpha` line is presentation. Nothing there may
    /// write simulation state, and nothing there may consume an input edge.
    fn redraw(&mut self) {
        let (Some(renderer), Some(camera)) = (self.renderer.as_mut(), self.camera.as_mut()) else {
            return;
        };

        let frame = self.clock.tick();

        // Screen space becomes world space here, and only here. The camera owns
        // the mapping because it owns the angle; `sim` is handed a direction it
        // can integrate without knowing a screen exists.
        let (right, up) = camera.ground_basis();
        let to_world = |axis: Vec2| MoveDir::new(right * axis.x + up * axis.y);

        // Zero, one or several — a frame buys whole ticks and the remainder
        // waits.
        //
        // **Intent is sampled per tick, not per frame.** `sample` clears the
        // latched edges, so sampling once per frame means a frame that runs no
        // ticks consumes a keypress and discards it — and uncapped, most frames
        // run no ticks. Sampling here makes a press wait for a tick and gives it
        // to exactly one. The symptom otherwise is "the attack sometimes does
        // not come out", which points nowhere near the frame loop.
        for dt in self.accumulator.pending(frame) {
            // Requests wait through zero-tick frames and apply before this
            // tick's attack input. Drawing below cannot consume or apply one.
            if let Some(profile) = self.input.take_profile() {
                self.game.set_attack_profile(profile);
            }
            let intent = self.input.sample();
            // `just_pressed`, not `held`: a swing is an edge. Holding the key
            // must not swing every tick, and a tap shorter than a frame must
            // still swing exactly once.
            self.game.step(
                dt,
                Intent::new(
                    to_world(intent.move_axis()),
                    intent.just_pressed(Action::Attack),
                )
                .with_interact(intent.just_pressed(Action::Interact)),
            );
            self.attack_effects.observe(&self.game);
        }

        // Blend the previous and current snapshots using the fractional tick remainder.
        // Everything below draws; nothing below simulates.
        let alpha = self.accumulator.alpha();
        self.attack_effects.rebuild(self.game.tick(), alpha);
        let player = self.game.player_presentation(alpha);
        let presentation_seconds = presentation_seconds(self.game.tick(), alpha);

        // Presentation, in three ways at once. **After** the step, or it would
        // add a frame of lag on top of the smoothing that is there on purpose.
        // The **drawn** position and the **frame's** delta, not the tick's,
        // because a rig whose whole job is smoothness must not be given a
        // stair-step to follow. And `held`, never `sample` — a camera that
        // consumed a keypress would be the same bug wearing a different hat.
        let facing = to_world(self.input.held().move_axis());
        camera.follow(player.ground_position(), facing, frame);

        if let Some(horde) = &mut self.horde_preview {
            horde.rebuild(self.game.enemy_presentations(alpha), presentation_seconds);
        }
        if let Some(character) = &mut self.character_preview {
            character.sample(player, alpha, presentation_seconds);
        }
        let world_assets = self.world_assets.as_mut().expect("rendering requires world assets");
        world_assets.rebuild(self.game.prop_presentations(alpha));
        self.collision_debug.rebuild(&self.game);

        // The overlay is built here rather than inside `render` for the same
        // reason asset placements are: what a readout says is a decision this crate
        // makes, and the renderer's job stops at drawing the rectangles it is
        // handed. Scoped so the sink's borrow ends before the draw.
        {
            let mut sink = self.quads.sink();
            let size = self.window.as_ref().expect("rendering has a window").inner_size();
            hud::draw(
                renderer.glyphs(),
                self.input.menu(),
                self.game.attack_status(),
                self.game.selected_scene_name().unwrap_or("--"),
                Vec2::new(size.width as f32, size.height as f32),
                &mut sink,
            );
        }

        // Count presentation, not redraw attempts: occluded surfaces can skip
        // thousands of attempts a second without producing any frames.
        let preview_instances = [preview_instance()];
        let meshes = world_assets.draws(self.asset_preview.as_ref(), &preview_instances);
        let character = self
            .character_preview
            .as_ref()
            .map(|preview| preview.draw(player));
        let horde = self.horde_preview.as_ref().map(LoadedHorde::draw);
        match renderer.render(
            camera,
            &meshes,
            character,
            horde,
            self.quads.as_slice(),
            self.collision_debug.drawings(),
            self.attack_effects.vertices(),
            self.capture.path(),
        ) {
            FrameOutcome::Presented { capture } => {
                self.frames += 1;
                if let Some(result) = capture {
                    self.capture.finish(result);
                }
            }
            FrameOutcome::Skipped => self.skipped += 1,
        }

        if self.clock.hud_due()
            && let Some(window) = &self.window
        {
            window.set_title(&format!(
                "arpg — {:.2}ms  {:.0}fps  N={}  ({} instances){}",
                self.clock.frame_ms(),
                self.clock.fps(),
                self.game.enemy_count(),
                world_assets.instance_count()
                    + self.collision_debug.drawings().len()
                    + usize::from(self.asset_preview.is_some())
                    + usize::from(self.character_preview.is_some())
                    + self.horde_preview.as_ref().map_or(0, LoadedHorde::instance_count),
                if renderer.vsync() { "  [vsync]" } else { "  [uncapped]" },
            ));
        }
    }

    pub(crate) fn run() {
        let event_loop = EventLoop::new().expect("create event loop");

        // Poll rather than Wait: never block waiting for input, just keep
        // looping. Vsync in the surface config is what actually paces us.
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut app = App::default();
        app.start_playtest(Scene::boot()).expect("valid built-in boot scene");
        event_loop.run_app(&mut app).expect("run app");
    }
}

impl ApplicationHandler for App {
    /// Initialize once when winit makes the event loop active. This native-only
    /// app keeps its window and GPU resources across redundant resume events.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return; // Can fire more than once; only build GPU state the first time.
        }

        let attrs = Window::default_attributes()
            .with_title("arpg")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));

        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let display_handle = event_loop.owned_display_handle();

        // Native-only, so we can simply block until the GPU is ready. The
        // cross-platform examples route this back through the event loop
        // because the browser forbids blocking the main thread.
        self.renderer = Some(pollster::block_on(Renderer::new(
            window.clone(),
            display_handle,
        )));
        self.world_assets = Some(WorldAssets::load(|mesh| {
            self.renderer.as_ref().expect("renderer was initialized")
                .upload_mesh(mesh).map_err(|error| error.to_string())
        }).expect("load default world assets"));
        // Required assets load before the first frame or harness command. Failure
        // is explicit; an invalid asset must never silently become a cube.
        self.show_character(default_character_path())
            .expect("load default player character");
        self.show_horde(default_horde_path())
            .expect("load default horde character");
        let size = window.inner_size();

        // Start framed on the character without a camera fly-in.
        let mut camera = OrthoCamera::new(size.width, size.height);
        camera.snap_to(self.game.player_pos());

        self.camera = Some(camera);
        self.window = Some(window);
        self.harness = harness::start();
        // GPU and asset loading are startup work, not elapsed play time.
        self.clock = Clock::default();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.renderer.is_none() || self.camera.is_none() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            // A key-up goes to whoever has focus. Alt-tab while running and the
            // release never arrives, so without this the character keeps going.
            WindowEvent::Focused(focused) => {
                log::debug!("focus: {focused}");
                if !focused {
                    self.input.release_all();
                }
            }

            WindowEvent::KeyboardInput { event: key_event, .. } => {
                log::debug!(
                    "key: physical={:?} state={:?} repeat={}",
                    key_event.physical_key,
                    key_event.state,
                    key_event.repeat
                );

                let PhysicalKey::Code(key) = key_event.physical_key else {
                    return;
                };
                let pressed = key_event.state == ElementState::Pressed;

                // The harness uses this same route. A modal UI event must not
                // also fire a debug command (especially Escape -> quit).
                let consumed = self.handle_key(key, pressed, key_event.repeat);

                if !consumed && pressed && !key_event.repeat {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::KeyV => {
                            self.renderer.as_mut().expect("initialized").toggle_vsync();
                        }
                        KeyCode::KeyP => {
                            let path = capture_dir().join(format!("arpg-{:04}.png", self.captures));
                            self.captures += 1;
                            self.capture.request(path, None, std::time::Instant::now());
                        }
                        _ => self.on_debug_key(key),
                    }
                }
            }

            WindowEvent::Resized(size) => {
                self.renderer.as_mut().expect("initialized").resize(size.width, size.height);
                self.camera.as_mut().expect("initialized").set_viewport(size.width, size.height);
            }

            WindowEvent::RedrawRequested => self.redraw(),

            _ => {}
        }
    }

    /// Polling alone does not request frames. Keep requesting redraws when
    /// the event queue drains, even when there has been no input.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Release expired keys before accepting new requests. A new tap stays
        // down until the next event-loop turn; its action edge remains latched
        // until a simulation tick samples it, even across zero-tick redraws.
        self.capture.expire(std::time::Instant::now());
        self.service_schedule();
        if self.drain_harness() {
            event_loop.exit();
            return;
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
