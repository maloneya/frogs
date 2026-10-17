//! Rendering validated assets and [`arpg_core::Instance`] placements.

mod camera;
mod capture;
mod character;
mod instance;
mod debug;
mod effect;
mod frame;
mod material;
mod lighting;
mod shadow;
mod visibility;
mod mesh;
mod overlay;
mod quad;
mod text;
mod timing;

pub use camera::OrthoCamera;
pub use arpg_core::PointLight;
pub use debug::{DebugOutline, OutlineShape};
pub use effect::{EffectVertex, MAX_EFFECT_VERTICES};
pub use frame::RenderFrame;
pub use timing::{GPU_TIMING_SLOTS, GpuCompletion, GpuTimings, RenderTimings};
pub use character::{
    CharacterBucket, CharacterHorde, CharacterMesh, CharacterPreview, MAX_HORDE_POSE_BUCKETS,
};
pub use mesh::{MeshAsset, MeshBatch, MeshUploadError};
pub use quad::{MAX_QUADS, Quad, QuadBuffer, QuadSink};
pub use text::Glyphs;


use std::sync::Arc;

use wgpu::CurrentSurfaceTexture;
use winit::window::Window;

use camera::CameraBinding;
use capture::Readback;
use character::CharacterPipeline;
use material::MaterialLayout;
use mesh::MeshPipeline;
use overlay::QuadPipeline;

/// Preserve 32-bit depth precision; stencil lets grounding follow all opaque
/// geometry while retaining its exclusion from character-covered samples.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32FloatStencil8;
// Keep fragment-read scene resources separate for adjacent GPU frames.
const SCENE_FRAME_SLOTS: usize = 3;

/// GPU device, surface and rendering resources.
pub struct Renderer {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Device identity for comparable benchmark artifacts; no runtime decisions.
    adapter_info: wgpu::AdapterInfo,
    depth: wgpu::TextureView,
    cameras: [CameraBinding; SCENE_FRAME_SLOTS],
    camera_slot: usize,
    static_meshes: MeshPipeline,
    debug: debug::DebugPipeline,
    effects: effect::EffectPipeline,
    weather: effect::EffectPipeline,
    character_preview: CharacterPipeline,
    /// The rasterised font, and the pipeline that draws what it lays out.
    ///
    /// Both live here rather than in `app` because a glyph's place in the atlas
    /// is a fact about this renderer's texture, and handing that outward would
    /// make every caller a second authority on where the letters are.
    font: text::Font,
    overlay: QuadPipeline,
    /// The best uncapped mode this surface supports, if any.
    uncapped: Option<wgpu::PresentMode>,
    /// Private because it mirrors state that actually lives in `config.present_mode`.
    /// A direct write would desync the two: the HUD would claim one thing while
    /// the surface did another, with no reconfigure to make it true.
    vsync: bool,
    gpu_timer: Option<timing::GpuTimer>,
    gpu_timing_requested: bool,
}

/// Whether rendering presented a frame, and the outcome of a requested PNG write.
/// A capture failure does not turn a presented frame into a skipped frame.
#[must_use = "frame counts and capture replies must follow the render outcome"]
pub enum FrameOutcome {
    /// No frame was presented; the caller may retry its capture request.
    Skipped,
    /// A frame was presented, with a result only when capture was requested.
    Presented {
        /// The synchronous readback and PNG write result.
        capture: Option<Result<(), String>>,
    },
}

/// Rendering outcome with CPU spans for the same call.
#[must_use = "frame counts and capture replies must follow the render outcome"]
pub struct RenderObservation {
    /// Whether presentation succeeded and any capture result.
    pub outcome: FrameOutcome,
    /// CPU timings, which may include waits but never claim GPU duration.
    pub cpu: RenderTimings,
    /// Whether this frame acquired a GPU timestamp slot.
    pub gpu_sampled: bool,
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
    /// Brings up the GPU: adapter, device, queue, swapchain and pipelines.
    ///
    /// Blocks until the device is ready. That is fine here and would not be on
    /// the web, which is why the wgpu examples route this through the event loop.
    pub async fn new(
        window: Arc<Window>,
        display_handle: winit::event_loop::OwnedDisplayHandle,
        gpu_timing_requested: bool,
    ) -> Self {
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

        let gpu_features = if gpu_timing_requested {
            adapter.features() & wgpu::Features::TIMESTAMP_QUERY
        } else { wgpu::Features::empty() };
        if gpu_timing_requested && gpu_features.is_empty() {
            log::warn!("GPU pass timestamps are unsupported by this adapter");
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("device"),
                required_features: gpu_features | wgpu::Features::DEPTH32FLOAT_STENCIL8,
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

        // So the app can screenshot itself. Requested rather than assumed: a
        // surface is only guaranteed to support RENDER_ATTACHMENT, and silently
        // configuring an unsupported usage is a validation error at the worst
        // possible moment.
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        } else {
            log::warn!("surface cannot be copied from; screenshots unavailable");
        }

        surface.configure(&device, &config);

        let depth = create_depth(&device, config.width, config.height);
        let cameras = std::array::from_fn(|_| CameraBinding::new(&device));
        let camera = &cameras[0];
        let material_layout = MaterialLayout::new(&device);
        let static_meshes = MeshPipeline::new(
            &device,
            &camera.layout,
            material_layout.clone(),
            config.format,
            DEPTH_FORMAT,
        );
        let character_preview = CharacterPipeline::new(
            &device,
            &camera.layout,
            material_layout,
            config.format,
            DEPTH_FORMAT,
        );
        let weather = effect::EffectPipeline::new(&device, &camera.layout, config.format, DEPTH_FORMAT);
        let effects = effect::EffectPipeline::new(&device, &camera.layout, config.format, DEPTH_FORMAT);
        let debug = debug::DebugPipeline::new(&device, &camera.layout, config.format, DEPTH_FORMAT);
        let font = text::Font::new(&device, &queue);
        let overlay = QuadPipeline::new(&device, &font, config.format, DEPTH_FORMAT);

        let gpu_timer = (!gpu_features.is_empty()).then(|| timing::GpuTimer::new(&device, &queue));
        Self {
            window,
            instance,
            surface,
            device,
            queue,
            config,
            adapter_info: info,
            depth,
            cameras,
            camera_slot: 0,
            static_meshes,
            debug,
            effects,
            weather,
            character_preview,
            font,
            overlay,
            uncapped,
            vsync: true,
            gpu_timer,
            gpu_timing_requested,
        }
    }

    /// Reconfigures the swapchain and rebuilds the depth buffer to match.
    ///
    /// Both, always: a depth attachment whose dimensions disagree with the
    /// colour attachment is a validation error, and dragging a window edge is
    /// the fastest way to find out.
    pub fn resize(&mut self, width: u32, height: u32) {
        // A zero-sized surface is invalid; minimising a window will produce one.
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth = create_depth(&self.device, self.config.width, self.config.height);
    }

    /// Uploads one already validated CPU mesh without performing file I/O.
    pub fn upload_mesh(
        &self,
        mesh: &arpg_assets::StaticMesh,
    ) -> Result<MeshAsset, MeshUploadError> {
        self.static_meshes
            .upload_mesh(&self.device, &self.queue, mesh)
    }

    /// Uploads one validated hierarchical character without performing file I/O.
    pub fn upload_character(
        &self,
        character: &arpg_assets::CharacterAsset,
    ) -> Result<CharacterMesh, MeshUploadError> {
        self.character_preview
            .upload_character(&self.device, &self.queue, character)
    }

    /// Where the letters are, for laying text out into a [`QuadSink`].
    ///
    /// **The metrics, never the `Font` that owns them.** `Font` holds a texture
    /// view and a sampler that nothing outside this crate may touch, and the
    /// GPU-free half is exactly what a caller needs — which is the statement
    /// the [`Glyphs`]/`Font` split exists to make.
    pub fn glyphs(&self) -> &Glyphs {
        self.font.glyphs()
    }

    /// Whether the surface is currently pinned to the refresh rate.
    pub fn vsync(&self) -> bool {
        self.vsync
    }

    /// Actual device and surface settings for performance comparisons.
    pub fn report_performance_context(&self, out: &mut arpg_core::Report) {
        out.text("name", &self.adapter_info.name);
        out.text("backend", &format!("{:?}", self.adapter_info.backend));
        out.text("driver", &self.adapter_info.driver);
        out.text("driver_info", &self.adapter_info.driver_info);
        out.text("present_mode", &format!("{:?}", self.config.present_mode));
        out.int("desired_maximum_frame_latency", u64::from(self.config.desired_maximum_frame_latency));
        out.text("format", &format!("{:?}", self.config.format));
        out.bool("timing_requested", self.gpu_timing_requested);
        out.bool("timing_available", self.gpu_timing_available());
        if self.gpu_timing_available() { out.num("timestamp_period_ns", self.queue.get_timestamp_period()); }
    }

    /// Whether optional GPU timestamp queries were enabled on this device.
    pub fn gpu_timing_available(&self) -> bool { self.gpu_timer.is_some() }

    /// Progresses old readbacks without waiting; callbacks retain their submitting frame ID.
    pub fn collect_gpu_timings(&mut self, receive: impl FnMut(GpuCompletion)) {
        if let Some(timer) = &mut self.gpu_timer { timer.collect(&self.device, &self.queue, receive); }
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

    fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }

    /// Renders each nonempty static mesh batch in one instanced draw, and each
    /// character part in one draw per occupied pose bucket.
    ///
    /// Distinguishes skipped frames from presentation, including capture errors.
    /// `capture_path` requests a synchronous PNG write of this frame. On a
    /// skipped frame no write is attempted; the caller owns retry and timeout.
    pub fn render(
        &mut self,
        input: RenderFrame<'_>,
        capture_path: Option<&std::path::Path>,
        frame_id: u64,
    ) -> RenderObservation {
        let acquire_start = std::time::Instant::now();
        let skipped = || RenderObservation {
            outcome: FrameOutcome::Skipped,
            cpu: RenderTimings { acquire: acquire_start.elapsed(), ..Default::default() },
            gpu_sampled: false,
        };
        let RenderFrame {
            camera, lights, environment, meshes, character, horde, overlay, debug_outlines, effects, weather,
        } = input;

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
                return skipped();
            }

            CurrentSurfaceTexture::Outdated => {
                self.reconfigure();
                return skipped();
            }

            // The surface itself is gone and has to be recreated from scratch.
            CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(self.window.clone())
                    .expect("recreate surface");
                self.reconfigure();
                return skipped();
            }

            // Transient: the compositor isn't ready or we're hidden. Drop the
            // frame; we'll be asked again immediately.
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
                return skipped();
            }

            CurrentSurfaceTexture::Validation => {
                unreachable!("no error scope registered, so validation errors panic instead")
            }
        };

        let mut cpu = RenderTimings { acquire: acquire_start.elapsed(), ..Default::default() };
        let upload_start = std::time::Instant::now();
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Nothing here talks to the GPU yet. We're recording into a command
        // buffer; it only becomes real work at `queue.submit`.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        let scene = &mut self.cameras[self.camera_slot];
        scene.upload(&self.queue, camera);
        scene.upload_lights(&self.queue, lights, environment, !weather.is_empty());
        self.static_meshes.upload(&self.queue, meshes);
        let weather_count = self.weather.upload(&self.queue, weather);
        let effect_count = self.effects.upload(&self.queue, effects);
        let debug_count = self.debug.upload(&self.queue, debug_outlines);
        scene.shadows.update(&self.queue, camera, environment, lights);
        let mut views = [Some(visibility::Frustum::new(camera.view_proj())), None, None];
        for (layer, matrix) in scene.shadows.matrices() {
            views[layer + 1] = Some(visibility::Frustum::new(matrix));
        }
        let character_draws = self.character_preview.upload(&self.device, &self.queue, character, horde, &views);
        let quads =
            self.overlay
                .upload(&self.queue, overlay, self.config.width, self.config.height);

        cpu.upload = upload_start.elapsed();
        let encode_start = std::time::Instant::now();
        let gpu_slot = self.gpu_timer.as_mut().and_then(|timer| timer.begin(frame_id));

        // Store only shadow depth: the surface pass still discards its depth.
        for (layer, view, binding) in scene.shadows.views() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if layer == 0 { "directional caster shadows" } else { "fire caster shadows" }), color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None,
                }), timestamp_writes: gpu_slot.map(|slot| self.gpu_timer.as_mut().expect("timing slot").writes(slot, layer)),
                occlusion_query_set: None, multiview_mask: None,
            });
            self.static_meshes.draw_depth(&mut pass, binding, meshes);
            self.character_preview.draw_depth(&mut pass, binding, character, horde, &character_draws, layer);
        }

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
                        // Geometry receives scene lighting; the space beyond it stays black.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1), store: wgpu::StoreOp::Discard }),
                }),
                timestamp_writes: gpu_slot.map(|slot| self.gpu_timer.as_mut().expect("timing slot").writes(slot, 2)),
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.static_meshes.draw_batches(&mut pass, scene, meshes);
            if let Some(horde) = horde {
                self.character_preview.draw_horde(
                    &mut pass,
                    scene,
                    horde.mesh(),
                    &character_draws,
                );
            }
            if let Some(character) = character {
                self.character_preview.draw_player(
                    &mut pass,
                    scene,
                    character.mesh(),
                    &character_draws,
                );
            }

            self.character_preview.draw_shadows(&mut pass, scene, &character_draws);

            // After the world and inside the same pass. The overlay neither
            // reads nor writes depth, so joining the pass costs nothing and
            // saves storing and reloading the whole frame between two of them.
            self.effects.draw(&mut pass, scene, effect_count);
            self.weather.draw(&mut pass, scene, weather_count);
            self.debug.draw(&mut pass, scene, debug_count);
            self.overlay.draw(&mut pass, quads);
        }

        // The frame is a copyable texture only between drawing and presenting,
        // so the copy is recorded into this same encoder.
        let capture = capture_path.map(|path| {
            let readback = Readback::new(&self.device, self.config.width, self.config.height);
            readback.record(&mut encoder, &frame.texture);
            (readback, path)
        });

        cpu.encode = encode_start.elapsed();
        let submit_start = std::time::Instant::now();
        self.queue.submit(Some(encoder.finish()));
        if let Some(slot) = gpu_slot {
            self.gpu_timer.as_ref().expect("timing slot").submitted(slot, &self.queue);
        }
        cpu.submit = submit_start.elapsed();
        let capture_start = std::time::Instant::now();

        let capture = capture.map(|(readback, path)| {
            readback.write_png(&self.device, path).map_err(|error| error.to_string())
        });

        cpu.capture = capture_start.elapsed();
        let present_start = std::time::Instant::now();
        // Tells winit we're about to present, so it can time its own bookkeeping
        // against the flip instead of guessing.
        self.window.pre_present_notify();
        self.queue.present(frame);
        cpu.present = present_start.elapsed();
        // Queued writes preserve GPU ordering when a slot wraps; no CPU wait.
        self.camera_slot = (self.camera_slot + 1) % SCENE_FRAME_SLOTS;
        RenderObservation { outcome: FrameOutcome::Presented { capture }, cpu, gpu_sampled: gpu_slot.is_some() }
    }
}

/// Surface-free GPU validation and pixel tests; a real adapter is required.
#[cfg(test)]
mod tests {
    use arpg_core::{LinearRgb, LinearRgba};
    use super::*;
    use arpg_core::Instance;
    use glam::Vec3;

    /// Matches the real swapchain format so the pipeline under test is the one
    /// that actually ships.
    const TEST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

    pub(crate) fn headless_device() -> (wgpu::Device, wgpu::Queue) {
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
            required_features: wgpu::Features::DEPTH32FLOAT_STENCIL8,
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("create headless device")
    }

    /// A broad white light keeps geometry/texture checks independent of torch tuning.
    fn lit_camera_binding(device: &wgpu::Device, queue: &wgpu::Queue) -> CameraBinding {
        let binding = CameraBinding::new(device);
        binding.upload_lights(queue, &[PointLight::new(
            Vec3::new(40.0, 100.0, 30.0), LinearRgb::WHITE, 12_000.0, 1_000.0,
        )], arpg_core::EnvironmentLight::try_new(LinearRgb::WHITE, 0.0, LinearRgb::WHITE, 0.0, Vec3::Y).unwrap(), false);
        binding
    }

    #[test]
    fn scene_binding_ring_matches_serialized_frames_across_two_queued_wraps() {
        const N: u32 = 128;
        let (device, queue) = headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut ring: [CameraBinding; SCENE_FRAME_SLOTS] = std::array::from_fn(|_| CameraBinding::new(&device));
        let pipeline = MeshPipeline::new(&device, &ring[0].layout, MaterialLayout::new(&device), TEST_FORMAT, DEPTH_FORMAT);
        let ground = arpg_assets::import_glb(include_bytes!("../../../assets/world/ground.glb")).unwrap();
        let prop = arpg_assets::import_glb(include_bytes!("../../../assets/world/prop.glb")).unwrap();
        let floor = pipeline.upload_mesh(&device, &queue, &ground).unwrap();
        let caster = pipeline.upload_mesh(&device, &queue, &prop).unwrap();
        let colour = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("queued scene ring evidence"),
            size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: TEST_FORMAT, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = colour.create_view(&Default::default());
        let depth = create_depth(&device, N, N);
        let render = |binding: &mut CameraBinding, frame: usize, cast: bool| {
            let phase = frame as f32;
            let mut camera = OrthoCamera::new(N, N);
            camera.zoom_by(0.30 + phase * 0.012);
            camera.snap_to(Vec3::new((phase - 3.0) * 0.22, 0.0, phase * 0.08));
            let environment = arpg_core::EnvironmentLight::try_new(
                LinearRgb::new(0.75, 0.85, 1.0), 0.1, LinearRgb::WHITE,
                if frame % 3 == 1 { 0.0 } else { 0.7 }, Vec3::new(-0.6, 0.8, 0.15 + phase * 0.08)).unwrap();
            let lights = [PointLight::new(Vec3::new(-2.0 + phase * 0.45, 3.0, -1.0 + phase * 0.2),
                if frame.is_multiple_of(2) { LinearRgb::new(1.0, 0.4, 0.15) } else { LinearRgb::new(0.25, 0.5, 1.0) },
                35.0, 10.0)];
            let floor_instances = [Instance::new(Vec3::ZERO, Vec3::new(24.0, 1.0, 24.0), LinearRgb::WHITE)];
            let caster_instances = [Instance::new(Vec3::new((phase - 3.0) * 0.15, 0.0, 0.0),
                Vec3::new(2.0, 4.0, 2.0), LinearRgb::WHITE).with_yaw(phase * 0.1)];
            let batches = [MeshBatch::new(&floor, &floor_instances), MeshBatch::new(&caster, &caster_instances)];
            binding.upload(&queue, &camera);
            binding.upload_lights(&queue, &lights, environment, !frame.is_multiple_of(2));
            binding.shadows.update(&queue, &camera, environment, &lights);
            pipeline.upload(&queue, &batches);
            let mut encoder = device.create_command_encoder(&Default::default());
            for (_, view, shadow_camera) in binding.shadows.views() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("queued scene shadow"), color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                });
                pipeline.draw_depth(&mut pass, shadow_camera, if cast { &batches[..] } else { &batches[..1] });
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("queued scene main"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view, depth_slice: None, resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                });
                // Only the receiver is visible, making the shadow content observable.
                pipeline.draw_batches(&mut pass, binding, &batches[..1]);
            }
            let readback = Readback::new(&device, N, N);
            readback.record(&mut encoder, &colour);
            queue.submit(Some(encoder.finish()));
            readback
        };
        let frames = SCENE_FRAME_SLOTS * 2 + 1;
        // Submit every frame, including both wraps, before the first CPU readback wait.
        let pending: Vec<_> = (0..frames).map(|frame|
            render(&mut ring[frame % SCENE_FRAME_SLOTS], frame, true)).collect();
        let actual: Vec<_> = pending.iter().map(|readback| readback.to_rgba(&device).unwrap()).collect();
        assert!(actual.windows(2).all(|pair| pair[0] != pair[1]), "test frames must visibly differ");
        let mut reference = CameraBinding::new(&device);
        for (frame, actual) in actual.iter().enumerate() {
            let expected = render(&mut reference, frame, true).to_rgba(&device).unwrap();
            assert_eq!(*actual, expected, "scene ring mixed frame resources at frame {frame}");
        }
        let unshadowed = render(&mut reference, 0, false).to_rgba(&device).unwrap();
        let shadow_pixels = actual[0].as_chunks::<4>().0.iter().zip(unshadowed.as_chunks::<4>().0)
            .filter(|(a, b)| a[..3] != b[..3]).count();
        assert!(shadow_pixels > 20, "queued references must exercise caster shadows");
        assert!(pollster::block_on(scope.pop()).is_none());
    }

    /// What a rendered silhouette looks like: its bounding box, and which way
    /// it leans.
    struct Silhouette {
        width: u32,
        height: u32,
        /// Signed pixel covariance distinguishes mirrored orientations with identical bounding boxes.
        lean: f32,
    }

    /// Renders a single instance at `yaw` and measures its silhouette.
    ///
    /// The body is a long thin bar — 8 units along its own +Z, half a unit the
    /// other ways — so what appears on screen says which way it is pointing.
    fn silhouette_of(yaw: f32) -> Silhouette {
        const N: u32 = 256;
        let (device, queue) = headless_device();

        let camera_binding = lit_camera_binding(&device, &queue);
        let pipeline = MeshPipeline::new(
            &device, &camera_binding.layout, MaterialLayout::new(&device), TEST_FORMAT, DEPTH_FORMAT,
        );
        let asset = arpg_assets::import_glb(include_bytes!("../../../assets/world/prop.glb")).unwrap();
        let mesh = pipeline.upload_mesh(&device, &queue, &asset).unwrap();

        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("silhouette target"),
            size: wgpu::Extent3d {
                width: N,
                height: N,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEST_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = create_depth(&device, N, N);

        camera_binding.upload(&queue, &OrthoCamera::new(N, N));

        // Stretch the half-metre prop into an eight-metre bar. The convention
        // test now exercises the production asset shader rather than a fallback.
        let instances = [Instance::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 16.0), LinearRgb::WHITE).with_yaw(yaw)];
        let batches = [MeshBatch::new(&mesh, &instances)];
        pipeline.upload(&queue, &batches);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("silhouette"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("silhouette pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pipeline.draw_batches(&mut pass, &camera_binding, &batches);
        }

        let readback = capture::Readback::new(&device, N, N);
        readback.record(&mut encoder, &color);
        queue.submit(Some(encoder.finish()));
        let rgba = readback.to_rgba(&device).expect("read the frame back");

        let (mut x0, mut y0, mut x1, mut y1) = (N, N, 0u32, 0u32);
        let mut lit = Vec::new();
        for y in 0..N {
            for x in 0..N {
                // Anything clearly brighter than the black clear colour.
                if rgba[((y * N + x) * 4) as usize] > 40 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                    lit.push((x as f32, y as f32));
                }
            }
        }
        assert!(!lit.is_empty(), "nothing was drawn at yaw {yaw}");

        let n = lit.len() as f32;
        let cx = lit.iter().map(|p| p.0).sum::<f32>() / n;
        let cy = lit.iter().map(|p| p.1).sum::<f32>() / n;
        let lean = lit.iter().map(|(x, y)| (x - cx) * (y - cy)).sum::<f32>() / n;

        Silhouette {
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
            lean,
        }
    }

    /// Checks yaw across Rust/WGSL: screen-right is yaw +3π/4, screen-up -3π/4.
    /// A sign error swaps the projected orientations without failing GPU validation.
    #[test]
    fn yaw_points_the_body_where_the_convention_says() {
        use std::f32::consts::PI;

        let east = silhouette_of(3.0 * PI / 4.0);
        let north = silhouette_of(-3.0 * PI / 4.0);

        assert!(
            east.width > east.height,
            "facing screen-right should be wide: {}x{}",
            east.width,
            east.height
        );
        assert!(
            north.height > north.width,
            "facing screen-up should be tall: {}x{}",
            north.width,
            north.height
        );
        assert!(
            east.width > north.width * 3,
            "the bar across the screen should be far wider: {} vs {}",
            east.width,
            north.width
        );

        // Foreshortening, from the other side: pointing away from the camera
        // costs the bar height, because the ground is compressed by sin(35.26).
        assert!(
            east.width > north.height,
            "an unforeshortened bar should out-measure a foreshortened one: {} vs {}",
            east.width,
            north.height
        );

        // Yaw 0 is the axis the convention is actually *defined* on, and it is
        // the one the two checks above cannot speak for: at odd multiples of
        // pi/4 a sin/cos swap is invisible to a bounding box, because it
        // reflects the bar onto a mirror image of the same size. Yaw 0 sends
        // the bar along world +Z, which leans the opposite way to the world +X
        // a swap would produce.
        let along_z = silhouette_of(0.0);
        let along_x = silhouette_of(PI / 2.0);
        assert!(
            along_z.lean * along_x.lean < 0.0,
            "yaw 0 and yaw pi/2 must lean opposite ways: {} vs {}",
            along_z.lean,
            along_x.lean
        );
        assert!(
            along_z.lean < 0.0,
            "yaw 0 sends the bar along world +Z, which leans up-to-the-right: {}",
            along_z.lean
        );
    }

    /// Renders one frame into an offscreen texture, with every validation error
    /// captured rather than left to the default handler.
    ///
    /// The asymmetric fixture must survive import, upload, projection, culling
    /// and shading as the metre-scale +Z-facing shape that was authored.
    /// Pipeline validation alone would accept an empty or reflected picture.
    #[test]
    fn the_static_preview_has_the_authored_silhouette() {
        const N: u32 = 256;
        let (device, queue) = headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let camera_binding = lit_camera_binding(&device, &queue);
        let pipeline = MeshPipeline::new(
            &device,
            &camera_binding.layout,
            MaterialLayout::new(&device),
            TEST_FORMAT,
            DEPTH_FORMAT,
        );
        let cpu = arpg_assets::import_glb(include_bytes!(
            "../../../assets/fixtures/static-preview.glb"
        ))
        .expect("fixture crosses the import boundary");
        let gpu = pipeline
            .upload_mesh(&device, &queue, &cpu)
            .expect("fixture uploads");

        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("asset preview silhouette"),
            size: wgpu::Extent3d {
                width: N,
                height: N,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEST_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = create_depth(&device, N, N);
        let mut camera = OrthoCamera::new(N, N);
        camera.zoom_by(0.125); // Clamp to a two-metre half-height.
        camera_binding.upload(&queue, &camera);
        pipeline.upload(&queue, &[MeshBatch::new(&gpu, &[Instance::new(Vec3::ZERO, Vec3::ONE, LinearRgb::WHITE)])]);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("asset preview silhouette"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("asset preview silhouette"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pipeline.draw(&mut pass, &camera_binding, &gpu, 0..1);
        }
        let readback = Readback::new(&device, N, N);
        readback.record(&mut encoder, &color);
        queue.submit(Some(encoder.finish()));
        let rgba = readback.to_rgba(&device).expect("read preview pixels");
        if let Some(error) = pollster::block_on(scope.pop()) {
            panic!("wgpu rejected the preview: {error}");
        }
        let (pixels, remainder) = rgba.as_chunks::<4>();
        assert!(remainder.is_empty(), "RGBA readback must contain whole pixels");
        let mut diagnostic = [(0_usize, 0_u64, 0_u64); 3];
        for (index, pixel) in pixels.iter().enumerate() {
            let class = if pixel[2].saturating_add(20) < pixel[0]
                && pixel[2].saturating_add(20) < pixel[1]
            {
                Some(0) // upper-left muted yellow
            } else if pixel[0].saturating_add(20) < pixel[1]
                && pixel[0].saturating_add(20) < pixel[2]
            {
                Some(1) // upper-right cyan
            } else if pixel[1].saturating_add(20) < pixel[0]
                && pixel[1].saturating_add(20) < pixel[2]
            {
                Some(2) // lower-left magenta
            } else {
                None
            };
            if let Some(class) = class {
                diagnostic[class].0 += 1;
                diagnostic[class].1 += (index as u32 % N) as u64;
                diagnostic[class].2 += (index as u32 / N) as u64;
            }
        }
        let yellow = pixels
            .iter()
            .filter(|pixel| pixel[2] < 10)
            .max_by_key(|pixel| pixel[1])
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .unwrap();
        assert!(
            diagnostic.iter().all(|class| class.0 > 500),
            "all three authored texture regions must survive sampling: {diagnostic:?}"
        );
        let centre = |class: (usize, u64, u64)| {
            (
                class.1 as f32 / class.0 as f32,
                class.2 as f32 / class.0 as f32,
            )
        };
        let (yellow_centre, cyan_centre, magenta_centre) = (
            centre(diagnostic[0]),
            centre(diagnostic[1]),
            centre(diagnostic[2]),
        );
        assert!(
            cyan_centre.1 < yellow_centre.1
                && magenta_centre.0 < cyan_centre.0
                && magenta_centre.1 > yellow_centre.1,
            "upper-left UV origin or texture orientation changed: {diagnostic:?}"
        );
        assert!(
            (45..=75).contains(&yellow[0]) && (100..=135).contains(&yellow[1]),
            "sRGB decoding and the quarter-red factor must happen in linear space: {yellow:?}"
        );

        let lit: Vec<(f32, f32)> = (0..N * N)
            .filter_map(|pixel| {
                let offset = (pixel * 4) as usize;
                (rgba[offset..offset + 3].iter().copied().max().unwrap_or(0) > 32)
                    .then_some(((pixel % N) as f32, (pixel / N) as f32))
            })
            .collect();
        assert!(
            lit.len() > 1_000,
            "a metre-scale solid should cover substantial pixels"
        );
        let (min_x, max_x, min_y, max_y) = lit.iter().fold(
            (
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ),
            |(min_x, max_x, min_y, max_y), &(x, y)| {
                (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
            },
        );
        let (width, height) = (max_x - min_x + 1.0, max_y - min_y + 1.0);
        assert!(
            (50.0..=100.0).contains(&width),
            "one metre projected to width {width}"
        );
        assert!(
            (50.0..=110.0).contains(&height),
            "one metre projected to height {height}"
        );

        let count = lit.len() as f32;
        let mean_x = lit.iter().map(|point| point.0).sum::<f32>() / count;
        let mean_y = lit.iter().map(|point| point.1).sum::<f32>() / count;
        let covariance = lit
            .iter()
            .map(|&(x, y)| (x - mean_x) * (y - mean_y))
            .sum::<f32>()
            / count;
        assert!(
            covariance < -20.0,
            "+Z asymmetry should lean up-right, got {covariance}"
        );
    }

    /// Empty batches and mixed meshes must preserve offsets and every placement.
    #[test]
    fn static_batches_preserve_meshes_offsets_and_all_instances() {
        const N: u32 = 256;
        let (device, queue) = headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let camera_binding = lit_camera_binding(&device, &queue);
        let pipeline = MeshPipeline::new(
            &device,
            &camera_binding.layout,
            MaterialLayout::new(&device),
            TEST_FORMAT,
            DEPTH_FORMAT,
        );
        let ground = arpg_assets::import_glb(include_bytes!("../../../assets/world/ground.glb")).unwrap();
        let prop = arpg_assets::import_glb(include_bytes!("../../../assets/world/prop.glb")).unwrap();
        let ground_gpu = pipeline.upload_mesh(&device, &queue, &ground).unwrap();
        let prop_gpu = pipeline.upload_mesh(&device, &queue, &prop).unwrap();

        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("static mesh batches"),
            size: wgpu::Extent3d {
                width: N,
                height: N,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEST_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = create_depth(&device, N, N);
        let mut camera = OrthoCamera::new(N, N);
        camera.zoom_by(0.125);
        camera_binding.upload(&queue, &camera);
        let floor = [Instance::new(Vec3::new(-1.0, 0.0, 1.0), Vec3::splat(0.8), LinearRgb::new(10.0, 0.0, 0.0))];
        let props = [
            Instance::new(Vec3::ZERO, Vec3::ONE, LinearRgb::new(0.0, 1.0, 0.0)),
            Instance::new(Vec3::new(1.0, 0.0, -1.0), Vec3::ONE, LinearRgb::new(0.0, 0.0, 1.0)),
        ];
        let batches = [
            MeshBatch::new(&prop_gpu, &[]),
            MeshBatch::new(&ground_gpu, &floor),
            MeshBatch::new(&ground_gpu, &[]),
            MeshBatch::new(&prop_gpu, &props),
        ];
        pipeline.upload(&queue, &batches);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("static mesh batches"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("static mesh batches"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pipeline.draw_batches(&mut pass, &camera_binding, &batches);
        }
        let readback = Readback::new(&device, N, N);
        readback.record(&mut encoder, &color);
        queue.submit(Some(encoder.finish()));
        let rgba = readback.to_rgba(&device).expect("read static pixels");
        if let Some(error) = pollster::block_on(scope.pop()) {
            panic!("wgpu rejected static batches: {error}");
        }

        let mut counts = [0_usize; 3];
        let mut x_sums = [0_usize; 3];
        for (index, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
            for channel in 0..3 {
                if pixel[channel] > 32 && pixel[(channel + 1) % 3] < 8 && pixel[(channel + 2) % 3] < 8 {
                    counts[channel] += 1;
                    x_sums[channel] += index % N as usize;
                }
            }
        }
        assert!(counts.iter().all(|count| *count > 60), "each mesh instance must remain visible: {counts:?}");
        let centres: [usize; 3] = std::array::from_fn(|i| x_sums[i] / counts[i]);
        assert!(centres[0] + 20 < centres[1] && centres[1] + 20 < centres[2], "batch offsets mixed placements: {centres:?}");
    }

    #[test]
    fn opaque_first_grounding_preserves_coverage_occlusion_and_distant_pixels() {
        #[derive(Clone, Copy)]
        enum Grounding { BeforeCharacters, AfterCharacters, Absent }
        struct Pixels { rgba: Vec<u8>, depth: Vec<u8>, stencil: Vec<u8> }
        let (device, queue) = headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut binding = CameraBinding::new(&device);
        let static_pipeline = MeshPipeline::new(&device, &binding.layout, MaterialLayout::new(&device), TEST_FORMAT, DEPTH_FORMAT);
        let ground = arpg_assets::import_glb(include_bytes!("../../../assets/world/ground.glb")).unwrap();
        let prop = arpg_assets::import_glb(include_bytes!("../../../assets/world/prop.glb")).unwrap();
        let floor = static_pipeline.upload_mesh(&device, &queue, &ground).unwrap();
        let foreground = static_pipeline.upload_mesh(&device, &queue, &prop).unwrap();
        let asset = arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/wolf/wolf.glb")).unwrap();
        let mut characters = CharacterPipeline::new(&device, &binding.layout, MaterialLayout::new(&device), TEST_FORMAT, DEPTH_FORMAT);
        let mesh = characters.upload_character(&device, &queue, &asset).unwrap();
        let floor_instances = [Instance::new(Vec3::ZERO, Vec3::new(24.0, 1.0, 24.0), LinearRgb::WHITE)];
        let foreground_instances = [Instance::new(Vec3::new(0.0, 0.0, -0.8), Vec3::new(0.5, 1.8, 0.5), LinearRgb::WHITE)];
        let batches = [MeshBatch::new(&floor, &floor_instances), MeshBatch::new(&foreground, &foreground_instances)];
        static_pipeline.upload(&queue, &batches);
        let lights = [PointLight::new(Vec3::new(-2.0, 3.0, -1.0), LinearRgb::new(1.0, 0.5, 0.1), 35.0, 16.0)];
        let environment = arpg_core::EnvironmentLight::try_new(LinearRgb::WHITE, 0.2, LinearRgb::WHITE, 0.7,
            Vec3::new(-0.6, 1.0, -0.3)).unwrap();
        for (width, height, close) in [(256, 256, false), (256, 256, true), (2560, 1440, false)] {
            let colour = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("grounding order oracle"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format: TEST_FORMAT, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = colour.create_view(&Default::default());
            let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("grounding oracle depth and stencil"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let depth = depth_texture.create_view(&Default::default());
            let mut camera = OrthoCamera::new(width, height);
            if close { camera.zoom_by(0.125); }
            binding.upload(&queue, &camera);
            for wet in [false, true] {
                binding.upload_lights(&queue, &lights, environment, wet);
                binding.shadows.update(&queue, &camera, environment, &lights);
                let mut views = [Some(visibility::Frustum::new(camera.view_proj())), None, None];
                for (layer, matrix) in binding.shadows.matrices() { views[layer + 1] = Some(visibility::Frustum::new(matrix)); }
                for (clip, seconds, root, pitch) in [("Run", 0.0, 0.0, 0.0), ("Run", 0.25, 0.004, 0.0),
                    ("Run", 0.125, 0.008, 0.0), ("Death", 0.5, 0.0, 0.0),
                    ("Run", 0.25, 0.8, 0.35), ("Death", 0.5, 0.004, 0.2)] {
                    let mut pose = asset.bind_pose();
                    assert!(asset.sample(asset.clip_named(clip).unwrap(), seconds, &mut pose));
                    let instances: Vec<_> = (0..25).map(|index| {
                        let position = Vec3::new((index % 5) as f32 * 0.4 - 0.8, root, (index / 5) as f32 * 0.4 - 0.8);
                        Instance::new(position, Vec3::ONE, LinearRgb::WHITE).with_yaw(index as f32 * 0.3).with_pitch(pitch)
                    }).collect();
                    let empty = CharacterBucket::new(&pose, &[]);
                    let mut buckets = [empty; MAX_HORDE_POSE_BUCKETS];
                    buckets[0] = CharacterBucket::new(&pose, &instances[..12]);
                    buckets[7] = CharacterBucket::new(&pose, &instances[12..]);
                    let horde = CharacterHorde::new(&mesh, buckets);
                    let player = CharacterPreview::new(&mesh, &pose,
                        Instance::new(Vec3::new(0.15, root, 0.15), Vec3::ONE, LinearRgb::new(0.4, 1.0, 0.5)).with_pitch(pitch));
                    let draws = characters.upload(&device, &queue, Some(player), Some(horde), &views);
                    let render = |order: Grounding| {
                        let mut encoder = device.create_command_encoder(&Default::default());
                        for (_, view, _) in binding.shadows.views() {
                            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("grounding oracle shadow initialization"), color_attachments: &[],
                                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                    view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None,
                                }), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                            });
                        }
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("grounding order oracle"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &view, depth_slice: None, resolve_target: None,
                                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                                })],
                                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                    // Store only for this oracle's exact geometry/coverage readback.
                                    view: &depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1), store: wgpu::StoreOp::Store }),
                                }), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                            });
                            static_pipeline.draw_batches(&mut pass, &binding, &batches);
                            if matches!(order, Grounding::BeforeCharacters) { characters.draw_shadows(&mut pass, &binding, &draws); }
                            characters.draw_horde(&mut pass, &binding, &mesh, &draws);
                            characters.draw_player(&mut pass, &binding, &mesh, &draws);
                            if matches!(order, Grounding::AfterCharacters) { characters.draw_shadows(&mut pass, &binding, &draws); }
                        }
                        let readback = Readback::new(&device, width, height);
                        readback.record(&mut encoder, &colour);
                        let copies = [(wgpu::TextureAspect::DepthOnly, 4), (wgpu::TextureAspect::StencilOnly, 1)].map(|(aspect, bytes)| {
                            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                                label: Some("grounding oracle attachment readback"), size: u64::from(width * height * bytes),
                                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
                            });
                            encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &depth_texture,
                                mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect },
                                wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout {
                                    offset: 0, bytes_per_row: Some(width * bytes), rows_per_image: Some(height) } },
                                wgpu::Extent3d { width, height, depth_or_array_layers: 1 });
                            buffer
                        });
                        queue.submit(Some(encoder.finish()));
                        let rgba = readback.to_rgba(&device).unwrap();
                        let [depth, stencil] = copies.map(|buffer| {
                            let (tx, rx) = std::sync::mpsc::channel();
                            buffer.map_async(wgpu::MapMode::Read, .., move |result| tx.send(result).unwrap());
                            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                            rx.recv().unwrap().unwrap();
                            let bytes = buffer.get_mapped_range(..).unwrap().to_vec();
                            buffer.unmap();
                            bytes
                        });
                        Pixels { rgba, depth, stencil }
                    };
                    let before = render(Grounding::BeforeCharacters);
                    let after = render(Grounding::AfterCharacters);
                    let absent = render(Grounding::Absent);
                    assert_eq!(before.depth, after.depth, "draw order changed geometry depth");
                    assert_eq!(after.depth, absent.depth);
                    assert_eq!(before.stencil, after.stencil, "draw order changed character coverage");
                    assert_eq!(after.stencil, absent.stencil);
                    let mut covered = 0;
                    let mut grounding = 0;
                    let mut changed = 0;
                    let mut maximum = 0;
                    for (((old, new), no_grounding), stencil) in before.rgba.as_chunks::<4>().0.iter()
                        .zip(after.rgba.as_chunks::<4>().0).zip(absent.rgba.as_chunks::<4>().0).zip(&after.stencil) {
                        if *stencil == 0 {
                            covered += 1;
                            assert_eq!(new, no_grounding, "grounding changed an opaque character pixel");
                        } else {
                            assert_eq!(old, new, "draw order changed uncovered floor/grounding pixels");
                            grounding += usize::from(old != no_grounding);
                        }
                        changed += usize::from(old != new);
                        maximum = maximum.max(old.iter().zip(new).map(|(a, b)| a.abs_diff(*b)).max().unwrap());
                    }
                    assert!(covered > 20, "oracle must exercise foreground character coverage");
                    if clip == "Run" && root == 0.0 { assert!(grounding > 0, "oracle must exercise visible grounding"); }
                    if width == 2560 {
                        // Opaque shading can round differently across orders.
                        // At the actual distant view, allow
                        // only sparse one-code-value differences; coverage,
                        // depth, grounding and character exclusion stay exact.
                        assert!(maximum <= 1 && changed * 100_000 <= (width * height) as usize,
                            "distant opaque shading changed: wet={wet} clip={clip} root={root} pitch={pitch} pixels={changed} max={maximum}");
                    }
                }
            }
        }
        assert!(pollster::block_on(scope.pop()).is_none());
    }

    /// A sampled pose crosses every character seam through real GPU draws:
    /// Blender's channels, joint indices and weights, CPU sampling, the uniform
    /// upload and WGSL skinning. A missing palette upload collapses these vertices
    /// to the origin, while a layout disagreement reaches the validation scope.
    fn render_character_bucket(bytes: &[u8], white_mix: f32, player: bool, seconds: f32) -> Vec<u8> {
        render_character_bucket_with_shadow(bytes, white_mix, player, seconds, None, false)
    }

    fn render_character_bucket_with_shadow(bytes: &[u8], white_mix: f32, player: bool, seconds: f32, shadow: Option<&[u8]>, union: bool) -> Vec<u8> {
        render_character_case(bytes, player, seconds, shadow, union, "Idle",
            Instance::new(Vec3::ZERO, Vec3::ONE, LinearRgb::WHITE).with_white_mix(white_mix))
    }

    fn render_character_case(bytes: &[u8], player: bool, seconds: f32, shadow: Option<&[u8]>,
        union: bool, clip: &str, instance: Instance) -> Vec<u8> {
        const N: u32 = 256;
        let (device, queue) = headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let camera_binding = lit_camera_binding(&device, &queue);
        let mut pipeline = CharacterPipeline::new(
            &device,
            &camera_binding.layout,
            MaterialLayout::new(&device),
            TEST_FORMAT,
            DEPTH_FORMAT,
        );
        let mut cpu = arpg_assets::import_character_glb(bytes)
        .expect("fixture crosses the character import boundary");
        if let Some(shadow) = shadow {
            cpu.attach_shadow(arpg_assets::import_character_glb(shadow).unwrap()).unwrap();
        }
        let gpu = pipeline
            .upload_character(&device, &queue, &cpu)
            .expect("character fixture uploads");

        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("character pose bucket"),
            size: wgpu::Extent3d {
                width: N,
                height: N,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEST_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = create_depth(&device, N, N);
        let mut camera = OrthoCamera::new(N, N);
        camera.zoom_by(0.125);
        camera_binding.upload(&queue, &camera);
        let mut pose = cpu.bind_pose();
        let idle = cpu
            .clip_named(clip)
            .expect("fixture carries its requested clip");
        assert!(cpu.sample(idle, seconds, &mut pose));
        let instances = [instance];
        let empty = CharacterBucket::new(&pose, &[]);
        let mut buckets = [empty; MAX_HORDE_POSE_BUCKETS];
        buckets[3] = CharacterBucket::new(&pose, &instances);
        let horde = CharacterHorde::new(&gpu, buckets);
        let character = CharacterPreview::new(&gpu, &pose, instances[0]);
        let mut draws = if player {
            pipeline.upload(&device, &queue, Some(character), None, &[Some(visibility::Frustum::new(camera.view_proj())), None, None])
        } else {
            pipeline.upload(&device, &queue, None, Some(horde), &[Some(visibility::Frustum::new(camera.view_proj())), None, None])
        };

        if union { draws.union_submissions(); }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("character pose bucket"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("character pose bucket"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1), store: wgpu::StoreOp::Discard }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if player {
                pipeline.draw_player(&mut pass, &camera_binding, &gpu, &draws);
            } else {
                pipeline.draw_horde(&mut pass, &camera_binding, &gpu, &draws);
            }
        }
        let readback = Readback::new(&device, N, N);
        readback.record(&mut encoder, &color);
        queue.submit(Some(encoder.finish()));
        let rgba = readback.to_rgba(&device).expect("read character pixels");
        if let Some(error) = pollster::block_on(scope.pop()) {
            panic!("wgpu rejected the character preview: {error}");
        }

        let lit: Vec<(u32, u32)> = (0..N * N)
            .filter_map(|pixel| {
                let offset = (pixel * 4) as usize;
                (rgba[offset..offset + 3].iter().copied().max().unwrap_or(0) > 32)
                    .then_some((pixel % N, pixel / N))
            })
            .collect();
        assert!(
            lit.len() > 1_000,
            "the skinned mannequin collapsed or disappeared: {} lit pixels",
            lit.len()
        );
        let bounds = lit.iter().fold(
            (N, 0_u32, N, 0_u32),
            |(min_x, max_x, min_y, max_y), &(x, y)| {
                (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
            },
        );
        let width = bounds.1 - bounds.0 + 1;
        let height = bounds.3 - bounds.2 + 1;
        let projected: Vec<_> = cpu.vertices().iter().map(|vertex| {
            let skinned = vertex.joints().into_iter().zip(vertex.weights()).fold(Vec3::ZERO, |sum, (joint, weight)|
                sum + pose.joint_matrices()[usize::from(joint)].matrix().transform_point3(vertex.position()) * weight);
            let world = glam::Quat::from_rotation_y(instance.yaw())
                * (glam::Quat::from_rotation_x(instance.pitch()) * (skinned * instance.scale())) + instance.pos();
            camera.view_proj().project_point3(world)
        }).collect();
        for (axis, actual) in [(0, width), (1, height)] {
            let min = projected.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min);
            let max = projected.iter().map(|p| p[axis]).fold(f32::NEG_INFINITY, f32::max);
            let expected = (max - min) * N as f32 * 0.5;
            assert!((actual as f32 - expected).abs() <= 6.0,
                "sampled silhouette axis {axis}: expected {expected:.1}px, got {actual}px from {bounds:?}");
        }
        rgba
    }

    #[test]
    fn shared_horde_skinning_matches_gpu_reference_for_materials_motion_and_instance_transforms() {
        let wolf = include_bytes!("../../../assets/characters/wolf/wolf.glb");
        let proxy = include_bytes!("../../../assets/characters/wolf/wolf.shadow.glb");
        let multipart = include_bytes!("../../../assets/fixtures/blender-multipart.glb");
        let instance = Instance::new(Vec3::new(0.05, 0.1, -0.05), Vec3::new(1.1, 0.9, 0.85), LinearRgb::WHITE)
            .with_pitch(0.35).with_yaw(0.6).with_white_mix(0.25);
        for (bytes, shadow, clip) in [(&wolf[..], None, "Run"), (&wolf[..], Some(&proxy[..]), "Run"),
            (&multipart[..], None, "Idle")] {
            for seconds in [0.0, 0.125, 0.25] {
                let reference = render_character_case(bytes, true, seconds, shadow, false, clip, instance);
                let actual = render_character_case(bytes, false, seconds, shadow, false, clip, instance);
                let changed = actual.iter().zip(&reference).filter(|(a, b)| a != b).count();
                let maximum = actual.iter().zip(&reference).map(|(&a, &b)| a.abs_diff(b)).max().unwrap();
                // CPU and Metal can contract floating-point operations differently.
                // Permit sparse one-code-value quantization changes, with exact silhouette coverage.
                for (a, b) in actual.as_chunks::<4>().0.iter().zip(reference.as_chunks::<4>().0) {
                    assert_eq!(a[..3] != [0; 3], b[..3] != [0; 3], "CPU skinning moved a silhouette edge");
                }
                assert!(maximum <= 1 && changed * 10_000 <= actual.len(),
                    "CPU/GPU difference exceeded numerical contract: clip={clip} t={seconds} proxy={} changed={changed} max={maximum}", shadow.is_some());
            }
        }
    }

    #[test]
    fn character_main_pixels_match_the_previous_union_submissions() {
        let enemy = include_bytes!("../../../assets/characters/wolf/wolf.glb");
        for player in [false, true] {
            for seconds in [0.0, 0.25] {
                assert_eq!(render_character_bucket(enemy, 0.0, player, seconds),
                    render_character_bucket_with_shadow(enemy, 0.0, player, seconds, None, true));
            }
        }
    }

    #[test]
    fn coarse_caster_keeps_the_visible_character_pixels_identical() {
        let enemy = include_bytes!("../../../assets/characters/wolf/wolf.glb");
        let shadow = include_bytes!("../../../assets/characters/wolf/wolf.shadow.glb");
        for seconds in [0.0, 0.5] {
            assert_eq!(render_character_bucket(enemy, 0.0, false, seconds),
                render_character_bucket_with_shadow(enemy, 0.0, false, seconds, Some(shadow), false));
        }
    }

    #[test]
    fn multipart_character_matches_the_unsplit_geometry_in_both_draw_paths() {
        let split = include_bytes!("../../../assets/fixtures/blender-multipart.glb");
        let reference = include_bytes!("../../../assets/fixtures/blender-multipart-reference.glb");
        for seconds in [0.0, 0.5] {
            let expected = render_character_bucket(reference, 0.0, true, seconds);
            for player in [true, false] {
                let actual = render_character_bucket(split, 0.0, player, seconds);
                // Equivalent geometry, UVs, material and pose should give the
                // same pixels despite separate mesh nodes and primitive order.
                assert_eq!(actual, expected, "split changed the image: player={player} t={seconds}");
            }
            for channel in 0..3 {
                let visible = expected.as_chunks::<4>().0.iter().filter(|pixel| {
                    pixel[channel] > 32 && (0..3).filter(|&other| other != channel).all(|other| pixel[other] < 4)
                }).count();
                assert!(visible > 100, "material {channel} missing: {visible} pixels");
            }
        }
    }

    #[test]
    fn a_character_pose_bucket_draws_its_instances_and_blends_to_white() {
        let normal = render_character_bucket(include_bytes!("../../../assets/fixtures/blender-bind-pose.glb"), 0.0, false, 0.5);
        let half = render_character_bucket(include_bytes!("../../../assets/fixtures/blender-bind-pose.glb"), 0.5, false, 0.5);
        let white = render_character_bucket(include_bytes!("../../../assets/fixtures/blender-bind-pose.glb"), 1.0, false, 0.5);
        let restored = render_character_bucket(include_bytes!("../../../assets/fixtures/blender-bind-pose.glb"), 0.0, false, 0.5);
        assert_eq!(normal, restored, "clearing the blend restores the original surface");
        let mut checked = 0;
        for ((normal, half), white) in normal.as_chunks::<4>().0.iter()
            .zip(half.as_chunks::<4>().0).zip(white.as_chunks::<4>().0) {
            if white[..3] == [0, 0, 0] {
                assert_eq!(normal, white, "flash must not change the silhouette");
                assert_eq!(half, white);
            } else {
                assert_eq!(&white[..3], &[255, 255, 255], "full blend ignores texture and lighting");
                for channel in 0..3 {
                    if normal[channel] < 200 {
                        assert!(half[channel] > normal[channel] && half[channel] < 255);
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 1000, "exercise the fade across the rendered surface");
    }

    /// Renders an overlay offscreen with GPU validation and pixel readback.
    fn overlay_frame(
        width: u32,
        height: u32,
        fill: impl FnOnce(&Glyphs, &mut QuadSink<'_>),
    ) -> Vec<u8> {
        let (device, queue) = headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let font = text::Font::new(&device, &queue);
        let overlay = QuadPipeline::new(&device, &font, TEST_FORMAT, DEPTH_FORMAT);

        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEST_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = create_depth(&device, width, height);

        let mut buf = QuadBuffer::default();
        {
            let mut sink = buf.sink();
            fill(font.glyphs(), &mut sink);
        }
        let count = overlay.upload(&queue, buf.as_slice(), width, height);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("overlay"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            overlay.draw(&mut pass, count);
        }

        let readback = capture::Readback::new(&device, width, height);
        readback.record(&mut encoder, &color);
        queue.submit(Some(encoder.finish()));
        let rgba = readback.to_rgba(&device).expect("read the frame back");

        if let Some(err) = pollster::block_on(scope.pop()) {
            panic!("wgpu rejected the overlay: {err}");
        }
        rgba
    }

    #[test]
    fn encoded_colour_is_decoded_before_linear_blending_and_srgb_output() {
        let rgba = overlay_frame(64, 32, |_, sink| {
            let grey = arpg_core::Srgb::from_rgb8([128; 3]).to_linear();
            sink.push(Quad::solid(glam::Vec4::new(0.0, 0.0, 32.0, 32.0), grey.with_alpha(1.0)));
            sink.push(Quad::solid(glam::Vec4::new(32.0, 0.0, 32.0, 32.0), grey.with_alpha(0.5)));
        });
        // Half coverage over black yields linear 0.10793, encoded as 92.
        for (x, expected) in [(16, 128_u8), (48, 92_u8)] {
            let offset = (16 * 64 + x) * 4;
            for channel in &rgba[offset..offset + 3] {
                assert!(channel.abs_diff(expected) <= 1, "expected {expected}, got {channel}");
            }
            assert_eq!(rgba[offset + 3], 255);
        }
    }

    /// An off-centre quad must occupy the expected screen band, leaving its mirror
    /// empty; this catches a pixel-to-clip Y sign error.
    #[test]
    fn an_overlay_quad_lands_where_it_was_placed() {
        const W: u32 = 256;
        const H: u32 = 128;
        // Deliberately nearer the top than the bottom.
        const RECT: (f32, f32, f32, f32) = (10.0, 8.0, 40.0, 16.0);

        let rgba = overlay_frame(W, H, |_, sink| {
            sink.push(Quad::solid(glam::Vec4::from(RECT), LinearRgba::WHITE));
        });
        let at = |x: u32, y: u32| rgba[((y * W + x) * 4) as usize];

        // **Fully opaque, not merely lit.** A `Quad::solid` samples the
        // reserved white texel, so anything less than saturated means the UV,
        // the packing or the sampler's filter mode disagree — a partially
        // covered smear would pass a "brighter than the clear colour" check.
        let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
        let (x1, y1) = (x0 + RECT.2 as u32 - 1, y0 + RECT.3 as u32 - 1);
        for (x, y) in [
            (x0, y0),
            (x1, y0),
            (x0, y1),
            (x1, y1),
            ((x0 + x1) / 2, (y0 + y1) / 2),
        ] {
            assert_eq!(
                at(x, y),
                u8::MAX,
                "the quad should cover ({x}, {y}) opaquely"
            );
        }

        // Just outside, on every side. A half-pixel offset in the projection
        // would still light the middle and fail here.
        for (x, y) in [(x0 - 1, y0), (x1 + 1, y0), (x0, y0 - 1), (x0, y1 + 1)] {
            assert_eq!(at(x, y), 0, "the quad should not reach ({x}, {y})");
        }

        // The mirror image, which is the whole point: a flipped y projection
        // draws here instead and passes every check above.
        let mirrored = H - y1 - 1;
        assert_eq!(
            at((x0 + x1) / 2, mirrored),
            0,
            "the quad is mirrored about the horizontal: y {mirrored} is lit"
        );
    }

    /// The atlas is actually sampled, and coverage actually reaches the frame.
    ///
    /// Separate from the placement test because it fails for different reasons:
    /// a broken UV, an unwritten texture, a sampler binding in the wrong slot,
    /// or a fragment shader reading the wrong channel all land here and none of
    /// them move a rectangle.
    #[test]
    fn text_puts_ink_on_the_frame() {
        const W: u32 = 256;
        const H: u32 = 64;

        let rgba = overlay_frame(W, H, |glyphs, sink| {
            glyphs.layout("HELLO", 8.0, 8.0, LinearRgba::WHITE, sink);
        });

        let ink = (0..W * H).filter(|i| rgba[(i * 4) as usize] > 40).count();
        assert!(ink > 100, "five letters should light more than {ink} pixels");

        // And not *everything*: a fragment shader that ignored coverage would
        // fill each glyph's whole rectangle, which is the failure that still
        // looks like text from far enough away.
        let area = (W * H) as usize;
        assert!(ink < area / 4, "{ink} of {area} pixels lit — coverage is being ignored");
    }
}
