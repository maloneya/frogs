use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use arpg_core::{InstanceBuffer, MoveDir};
use arpg_gfx::{OrthoCamera, Renderer};
use arpg_sim::World;

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
    /// Reused every frame so a steady state allocates nothing.
    instances: InstanceBuffer,
    input: Input,
    clock: Clock,
}

/// One notch of zoom per keypress.
const ZOOM_STEP: f32 = 1.2;

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
        self.camera = Some(OrthoCamera::new(size.width, size.height));
        self.window = Some(window);
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
                renderer.render(camera, self.instances.as_slice());

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
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
