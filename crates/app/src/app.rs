use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use arpg_core::{InstanceBuffer, MoveDir};
use arpg_gfx::{OrthoCamera, Renderer};
use arpg_sim::World;

use crate::harness::{self, Command, Request};
use crate::input::Input;
use crate::time::Clock;

/// Owns everything and wires it together. Deliberately the only place that
/// knows about all the subsystems at once — `gfx` and `world` stay ignorant of
/// each other, and this is where they meet.
#[derive(Default)]
pub(crate) struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    camera: Option<OrthoCamera>,
    world: World,
    /// Numbers the screenshots, so repeated captures do not overwrite.
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
    /// averages to it. It is also the only way to notice the app being throttled
    /// while it sits in the background.
    frames: u64,
    /// Present only when `ARPG_HARNESS` asked for a control socket.
    harness: Option<std::sync::mpsc::Receiver<Request>>,
    /// Keys to release, and replies to send, once the game clock reaches them.
    /// Deadlines are the game's own, so a `hold` lasts that long *in the world*.
    scheduled: Vec<(std::time::Instant, Option<KeyCode>, Option<std::sync::mpsc::Sender<String>>)>,
    /// Replies owed to callers waiting on a frame to be captured.
    awaiting_frame: Vec<std::sync::mpsc::Sender<String>>,
    /// Reused every frame so a steady state allocates nothing.
    instances: InstanceBuffer,
    input: Input,
    clock: Clock,
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

impl App {
    /// Debug and meta commands, kept deliberately apart from the action layer.
    ///
    /// These are not things the *character* does — they are things done to the
    /// running program, and the difference is not cosmetic. Game actions are
    /// sampled as state once per tick, need rebinding, and will one day come
    /// from a gamepad or a replay. These are one-shot, fire straight from the
    /// event callback, and are meaningless to a simulation. Funnelling them
    /// through `Action` would put "toggle vsync" in the vocabulary the horde's
    /// AI speaks.
    ///
    /// The two sets must stay disjoint; `BINDINGS` is the list to check against.
    fn on_debug_key(&mut self, key: KeyCode) {
        match key {
            // Doubling rather than stepping: the interesting range spans three
            // orders of magnitude, and the knee is easier to find by bisection
            // than by walking. Both directions clamp inside `set_enemy_count`.
            KeyCode::BracketRight => {
                self.world.set_enemy_count(self.world.enemy_count() * 2);
                log::info!("N = {}", self.world.enemy_count());
            }
            KeyCode::BracketLeft => {
                self.world.set_enemy_count(self.world.enemy_count() / 2);
                log::info!("N = {}", self.world.enemy_count());
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

    /// Applies whatever the control socket has sent since the last frame.
    ///
    /// Runs before input is sampled, so an injected key takes effect on the
    /// very frame it arrives rather than the one after.
    fn drain_harness(&mut self) -> bool {
        // Collected up front so the receiver borrow ends before the loop needs
        // the rest of `self`.
        let Some(rx) = &self.harness else { return false };
        let requests: Vec<Request> = rx.try_iter().collect();
        let mut quit = false;

        for Request { command, reply } in requests {
            let now = std::time::Instant::now();
            let answer = match command {
                Command::Press(key) => {
                    self.input.on_key(key, true, false);
                    "ok".to_string()
                }
                Command::Release(key) => {
                    self.input.on_key(key, false, false);
                    "ok".to_string()
                }
                Command::Tap(key) => {
                    self.input.on_key(key, true, false);
                    self.scheduled.push((now, Some(key), None));
                    "ok".to_string()
                }
                Command::Hold(key, ms) => {
                    self.input.on_key(key, true, false);
                    let due = now + std::time::Duration::from_millis(ms);
                    self.scheduled.push((due, Some(key), Some(reply)));
                    continue; // replies once the key comes back up
                }
                Command::Wait(ms) => {
                    let due = now + std::time::Duration::from_millis(ms);
                    self.scheduled.push((due, None, Some(reply)));
                    continue;
                }
                Command::Shot(path) => {
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.request_capture(path);
                    }
                    self.awaiting_frame.push(reply);
                    continue; // replies once the file exists
                }
                Command::State => self.report_state(),
                Command::SetEnemies(n) => {
                    self.world.set_enemy_count(n);
                    format!("enemies {}", self.world.enemy_count())
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

    /// One line of everything worth knowing, so a test can assert on numbers
    /// instead of inferring them from pixels.
    fn report_state(&self) -> String {
        let p = self.world.player_pos();
        let c = self.camera.as_ref().map(|c| c.target()).unwrap_or_default();
        format!(
            "player_pos {:.3} {:.3} {:.3} facing {:.4} camera_target {:.3} {:.3} enemies {} contacts {} instances {} frames {} skipped {} frame_ms {:.2} vsync {}",
            p.x,
            p.y,
            p.z,
            self.world.player_facing(),
            c.x,
            c.z,
            self.world.enemy_count(),
            self.world.contacts(),
            self.instances.as_slice().len(),
            self.frames,
            self.skipped,
            self.clock.frame_ms(),
            self.renderer.as_ref().is_some_and(Renderer::vsync),
        )
    }

    /// Fires the releases and replies whose deadline has passed.
    fn service_schedule(&mut self) {
        let now = std::time::Instant::now();
        let mut still_pending = Vec::new();

        for (due, key, reply) in std::mem::take(&mut self.scheduled) {
            if now >= due {
                if let Some(key) = key {
                    self.input.on_key(key, false, false);
                }
                if let Some(reply) = reply {
                    let _ = reply.send("ok".to_string());
                }
            } else {
                still_pending.push((due, key, reply));
            }
        }
        self.scheduled = still_pending;
    }

    pub(crate) fn run() {
        let event_loop = EventLoop::new().expect("create event loop");

        // Poll rather than Wait: never block waiting for input, just keep
        // looping. Vsync in the surface config is what actually paces us.
        event_loop.set_control_flow(ControlFlow::Poll);
        event_loop.run_app(&mut App::default()).expect("run app");
    }
}

impl ApplicationHandler for App {
    /// Window and GPU creation belongs here rather than in `main`, because on
    /// mobile platforms the surface is destroyed and rebuilt as the app moves
    /// in and out of the foreground. winit models that as suspend/resume, and
    /// guarantees `resumed` fires before any window event on every platform.
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
        let size = window.inner_size();

        // Start framed on the character rather than easing in from the origin.
        // Harmless today, since both begin there — but the moment anything
        // spawns the player elsewhere, the first thing the player would see is
        // the camera flying across the world to catch up.
        let mut camera = OrthoCamera::new(size.width, size.height);
        camera.snap_to(self.world.player_pos());

        self.camera = Some(camera);
        self.window = Some(window);
        self.harness = harness::start();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(renderer), Some(camera)) = (self.renderer.as_mut(), self.camera.as_mut()) else {
            return;
        };

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

                // Both halves see every event. Game actions need the release to
                // know a key came up; the debug commands only care about the
                // leading edge. Unbound keys fall through `on_key` untouched.
                self.input.on_key(key, pressed, key_event.repeat);

                if pressed && !key_event.repeat {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::KeyV => {
                            renderer.toggle_vsync();
                        }
                        KeyCode::KeyP => {
                            let path = capture_dir()
                                .join(format!("arpg-{:04}.png", self.captures));
                            self.captures += 1;
                            renderer.request_capture(path);
                        }
                        _ => self.on_debug_key(key),
                    }
                }
            }

            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
                camera.set_viewport(size.width, size.height);
            }

            WindowEvent::RedrawRequested => {
                let dt = self.clock.tick();

                // The per-frame spine: sample intent, resolve it against the
                // view, step, extract. A fixed-timestep loop lands around the
                // step later; nothing else here has to change for it.
                let axis = self.input.sample().move_axis();

                // Screen space becomes world space here, and only here. The
                // camera owns the mapping because it owns the angle; `sim` is
                // handed a direction it can integrate without knowing a screen
                // exists.
                let (right, up) = camera.ground_basis();
                let dir = MoveDir::new(right * axis.x + up * axis.y);

                self.world.step(dt, dir);

                // After the step, not before: following last tick's position
                // would add a frame of lag on top of the smoothing that is
                // there deliberately. Per frame rather than per tick, because
                // where the camera points is presentation, not simulation.
                camera.follow(self.world.player_pos(), dir, dt);

                self.world.extract(self.instances.sink());
                // Counted only when a frame actually reached the screen. An
                // occluded window skips the draw entirely, and counting those
                // would report thousands of frames a second for drawing nothing.
                if renderer.render(camera, self.instances.as_slice()) {
                    self.frames += 1;
                } else {
                    self.skipped += 1;
                }

                // The capture is written inside `render`, so anything waiting
                // on a screenshot can be told the file exists.
                for reply in std::mem::take(&mut self.awaiting_frame) {
                    let _ = reply.send("ok".to_string());
                }


                if self.clock.hud_due()
                    && let Some(window) = &self.window
                {
                    window.set_title(&format!(
                        "arpg — {:.2}ms  {:.0}fps  N={}  ({} instances){}",
                        self.clock.frame_ms(),
                        self.clock.fps(),
                        self.world.enemy_count(),
                        self.instances.as_slice().len(),
                        if renderer.vsync() { "  [vsync]" } else { "  [uncapped]" },
                    ));
                }
            }

            _ => {}
        }
    }

    /// winit is event-driven by default: with nothing happening, it sleeps. A
    /// game is the opposite — it must produce a frame whether or not anyone
    /// touched the keyboard. Requesting a redraw every time the event queue
    /// drains is what turns this into a continuous loop.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The control socket is drained here rather than inside the redraw,
        // because this is the one point in the loop where nothing else is
        // borrowed out of `self` — and it runs immediately before the frame,
        // so an injected key takes effect on the very next one.
        //
        // Expiries are serviced *first*, so a key pressed by this pass survives
        // until the next one and is therefore held for exactly one frame.
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
