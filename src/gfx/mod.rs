mod camera;
mod cube;
mod instance;

pub use camera::OrthoCamera;
pub use instance::{Instance, InstanceBuffer, InstanceSink, MAX_INSTANCES};

use std::sync::Arc;

use wgpu::CurrentSurfaceTexture;
use winit::window::Window;

use camera::CameraBinding;
use cube::CubePipeline;

/// Depth32Float is the safe universal choice. Under an orthographic camera
/// depth is linear, so we aren't fighting the precision crush that makes
/// perspective projections want 24-bit-plus formats.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

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
    depth: wgpu::TextureView,
    camera: CameraBinding,
    cubes: CubePipeline,
    /// The best uncapped mode this surface supports, if any.
    uncapped: Option<wgpu::PresentMode>,
    /// Private because it mirrors state that actually lives in `config.present_mode`.
    /// A direct write would desync the two: the HUD would claim one thing while
    /// the surface did another, with no reconfigure to make it true.
    vsync: bool,
}

/// The depth buffer must match the colour attachment's dimensions exactly, so
/// this is called from both setup and `resize`. Forgetting the resize path is
/// the classic way to crash on the first window drag.
fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
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

        // Vsync by default. Frame pacing is the foundation everything else gets
        // measured against, so we stay pinned to the refresh rate unless
        // explicitly asked otherwise.
        //
        // The exception is measurement: vsync *quantises* frame time to
        // multiples of the refresh interval, so under it a renderer that takes
        // 4ms and one that takes 16ms look identical. Finding where the horde
        // actually costs requires taking the cap off.
        let caps = surface.get_capabilities(&adapter);
        log::info!("present modes: {:?}", caps.present_modes);

        let uncapped = [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
            .into_iter()
            .find(|m| caps.present_modes.contains(m));

        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let depth = create_depth(&device, config.width, config.height);
        let camera = CameraBinding::new(&device);
        let cubes = CubePipeline::new(&device, &camera.layout, config.format, DEPTH_FORMAT);

        Self {
            window,
            instance,
            surface,
            device,
            queue,
            config,
            depth,
            camera,
            cubes,
            uncapped,
            vsync: true,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        // A zero-sized surface is invalid; minimising a window will produce one.
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth = create_depth(&self.device, self.config.width, self.config.height);
    }

    pub fn vsync(&self) -> bool {
        self.vsync
    }

    /// Returns whether vsync is on afterwards — unchanged if the surface has no
    /// uncapped mode to offer.
    pub fn toggle_vsync(&mut self) -> bool {
        let Some(uncapped) = self.uncapped else {
            log::warn!("no uncapped present mode available on this surface");
            return self.vsync;
        };

        self.vsync = !self.vsync;
        self.config.present_mode =
            if self.vsync { wgpu::PresentMode::AutoVsync } else { uncapped };
        self.reconfigure();
        self.vsync
    }

    fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self, camera: &OrthoCamera, instances: &[Instance]) {
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

        self.camera.upload(&self.queue, camera);
        let count = self.cubes.upload(&self.queue, instances);

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // LoadOp::Clear is free — the GPU fills the tile on the
                        // way in. Loading the previous contents instead would
                        // cost real bandwidth, so always clear what you'll
                        // fully overwrite.
                        //
                        // These are *linear* values. The surface is sRGB, so the
                        // hardware encodes on write: passing the sRGB numbers
                        // you actually want (0.05, 0.06, 0.08) would come out
                        // roughly five times too bright. This is the linear
                        // pre-image of that colour.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0039,
                            g: 0.0049,
                            b: 0.0072,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        // Discard, not Store: nothing reads the depth buffer
                        // after this pass, so writing it back to memory would
                        // be pure wasted bandwidth.
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.cubes.draw(&mut pass, &self.camera.bind_group, count);
        }

        self.queue.submit(Some(encoder.finish()));

        // Tells winit we're about to present, so it can time its own bookkeeping
        // against the flip instead of guessing.
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}

/// Headless GPU tests.
///
/// These exist to close the gap Rust cannot see across: the vertex layout in
/// `instance.rs` and the `@location` declarations in `shader.wgsl` are one
/// contract maintained in two files and two languages. `cargo check` is blind
/// to it. wgpu's validator is not — it compares them at pipeline creation and
/// again at draw time, so the job here is simply to reach those points without
/// a window and to turn any complaint into a test failure.
///
/// A device needs no surface: `compatible_surface: None`, and on Metal the
/// display handle goes unused.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::instance::InstanceBuffer;
    use glam::Vec3;

    /// Matches the real swapchain format so the pipeline under test is the one
    /// that actually ships.
    const TEST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

    fn headless_device() -> (wgpu::Device, wgpu::Queue) {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            ..Default::default()
        }))
        .expect("no GPU adapter available for tests");

        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("headless test device"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("create headless device")
    }

    /// Renders one frame into an offscreen texture, with every validation error
    /// captured rather than left to the default handler.
    ///
    /// Pipeline creation catches a Rust/WGSL mismatch — a changed `Instance`
    /// field, a stride that no longer matches, an attribute pointing at a
    /// `@location` the shader does not declare. Actually drawing catches the
    /// rest: buffer sizes, bind group layouts, and the depth attachment's
    /// dimensions disagreeing with the colour attachment.
    #[test]
    fn a_frame_renders_without_validation_errors() {
        let (device, queue) = headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let camera_binding = CameraBinding::new(&device);
        let cubes = CubePipeline::new(&device, &camera_binding.layout, TEST_FORMAT, DEPTH_FORMAT);

        const W: u32 = 256;
        const H: u32 = 128;

        let color = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("test colour target"),
                size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TEST_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth = create_depth(&device, W, H);

        let camera = OrthoCamera::new(W, H);
        camera_binding.upload(&queue, &camera);

        let mut buf = InstanceBuffer::default();
        let mut sink = buf.sink();
        for i in 0..64 {
            sink.push(Instance::new(
                Vec3::new(i as f32, 0.0, 0.0),
                Vec3::ONE,
                Vec3::new(0.3, 0.1, 0.05),
            ));
        }
        let count = cubes.upload(&queue, buf.as_slice());
        assert_eq!(count, 64, "upload should report every instance as live");

        let mut encoder = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("test frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("test pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            cubes.draw(&mut pass, &camera_binding.bind_group, count);
        }
        queue.submit(Some(encoder.finish()));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll device");

        if let Some(err) = pollster::block_on(scope.pop()) {
            panic!("wgpu rejected the frame: {err}");
        }
    }
}
