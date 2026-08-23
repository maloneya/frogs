use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use arpg_core::InstanceBuffer;
use arpg_gfx::{OrthoCamera, Renderer};
use crate::time::Clock;
use arpg_sim::World;

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
    clock: Clock,
}

/// One notch of zoom per keypress.
const ZOOM_STEP: f32 = 1.2;

impl App {
    fn on_key(&mut self, key: KeyCode) {
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

            WindowEvent::Focused(focused) => log::debug!("focus: {focused}"),

            WindowEvent::KeyboardInput { event: key_event, .. } => {
                log::debug!(
                    "key: physical={:?} state={:?} repeat={}",
                    key_event.physical_key,
                    key_event.state,
                    key_event.repeat
                );

                if key_event.state == ElementState::Pressed
                    && let PhysicalKey::Code(key) = key_event.physical_key
                {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::KeyV => {
                            renderer.toggle_vsync();
                        }
                        _ => self.on_key(key),
                    }
                }
            }

            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
                camera.set_viewport(size.width, size.height);
            }

            WindowEvent::RedrawRequested => {
                self.clock.tick();

                // The per-frame spine. Next chunk a fixed-timestep loop lands
                // between the tick and the extract.
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
