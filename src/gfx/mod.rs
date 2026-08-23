use std::sync::Arc;

use wgpu::CurrentSurfaceTexture;
use winit::window::Window;

/// Everything required to get pixels onto the screen.
///
/// The four wgpu handles are worth keeping straight, because the names are not
/// self-explanatory:
///
/// - `instance` is the library entry point. It enumerates what hardware exists.
/// - `adapter` is one physical GPU. We only need it during setup, to ask what
///   it's capable of, so it isn't stored.
/// - `device` is a logical handle to that GPU. Every resource (buffer, texture,
///   pipeline) is created from it.
/// - `queue` is the channel we submit finished command buffers on. This is the
///   actual boundary between our process and the GPU.
///
/// `surface` is the swapchain: a small ring of textures the OS compositor can
/// display. We render into one and hand it back to be presented.
pub struct Renderer {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, display_handle: winit::event_loop::OwnedDisplayHandle) -> Self {
        let size = window.inner_size();

        // `from_env` lets WGPU_BACKEND / WGPU_POWER_PREF override our choices at
        // runtime, which is handy for A/B-ing backends without a rebuild.
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display_handle)),
        );

        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                // Guarantees the adapter we get can actually draw to our window.
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("no suitable GPU adapter");

        let info = adapter.get_info();
        log::info!("adapter: {} ({:?}, {:?})", info.name, info.device_type, info.backend);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("device"),
                required_features: wgpu::Features::empty(),
                // Native-only, so take everything this GPU offers rather than
                // the portable baseline. We'll want the headroom for large
                // instance buffers later.
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("create device");

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface not supported by adapter");

        // Vsync. Frame pacing is the foundation everything else gets measured
        // against, so we start pinned to the display's refresh rate and only
        // change this deliberately, with a reason.
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        Self { window, instance, surface, device, queue, config }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        // A zero-sized surface is invalid; minimising a window will produce one.
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self) {
        // Acquiring a swapchain image can fail in several recoverable ways —
        // the window resized behind our back, the display changed, the GPU
        // dropped the surface. Each wants a slightly different response, and
        // none of them should crash the game.
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => frame,

            // Still usable, but no longer matches the window. Rebuild and skip
            // this frame rather than presenting something stretched.
            CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.reconfigure();
                return;
            }

            CurrentSurfaceTexture::Outdated => {
                self.reconfigure();
                return;
            }

            // The surface itself is gone and has to be recreated from scratch.
            CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(self.window.clone())
                    .expect("recreate surface");
                self.reconfigure();
                return;
            }

            // Transient: the compositor isn't ready or we're hidden. Drop the
            // frame; we'll be asked again immediately.
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return,

            CurrentSurfaceTexture::Validation => {
                unreachable!("no error scope registered, so validation errors panic instead")
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Nothing here talks to the GPU yet. We're recording into a command
        // buffer; it only becomes real work at `queue.submit`.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // LoadOp::Clear is free — the GPU fills the tile on the
                        // way in. Loading the previous contents instead would
                        // cost real bandwidth, so always clear what you'll
                        // fully overwrite.
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.05, g: 0.06, b: 0.08, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Nothing drawn yet — the clear is the whole frame for now.
        }

        self.queue.submit(Some(encoder.finish()));

        // Tells winit we're about to present, so it can time its own bookkeeping
        // against the flip instead of guessing.
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}
