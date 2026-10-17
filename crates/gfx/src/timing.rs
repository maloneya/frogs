//! Opt-in, bounded GPU observations. Readback never waits for the current frame.

use std::{sync::{Arc, atomic::{AtomicU8, Ordering}}, time::Duration};

/// CPU portions of a render call. GPU execution is measured separately.
#[derive(Clone, Copy, Default)]
pub struct RenderTimings {
    /// Waiting for or acquiring the next surface texture.
    pub acquire: Duration,
    /// Preparing and staging geometry, palettes, uniforms and overlays.
    pub upload: Duration,
    /// Recording render commands.
    pub encode: Duration,
    /// Finishing and submitting the command buffer, including any backpressure.
    pub submit: Duration,
    /// Synchronous screenshot readback and PNG writing, when requested.
    pub capture: Duration,
    /// Presentation notification and surface presentation.
    pub present: Duration,
}

/// Pass intervals from one submission, associated with its original frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTimings {
    /// Directional shadow, fire shadow, and main pass; absent passes stay absent.
    pub passes: [Option<Duration>; 3],
    /// Active passes whose timestamps were missing or discontinuous.
    pub unavailable: [bool; 3],
    /// Earliest pass start to latest end, only when all active passes were measured.
    pub span: Option<Duration>,
}

/// A completed asynchronous observation, including explicit readback failure.
#[derive(Clone, Copy, Debug)]
pub struct GpuCompletion {
    /// Caller-supplied identifier; never reused after a recorder reset.
    pub frame_id: u64,
    /// An error never masquerades as zero GPU time.
    pub result: Result<GpuTimings, &'static str>,
}

/// Maximum observations retained while sampling and readback finish asynchronously.
pub const GPU_TIMING_SLOTS: usize = 8;
const QUERIES: u32 = 6;
const BYTES: u64 = QUERIES as u64 * 8;
const RESOLVE_STRIDE: u64 = wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;

struct Slot {
    // Each pass owns its counter buffer; decode still validates every pair.
    queries: [wgpu::QuerySet; 3],
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    ready: Arc<AtomicU8>,
    frame: Option<u64>,
    passes: [bool; 3],
}

pub(crate) struct GpuTimer {
    slots: [Slot; GPU_TIMING_SLOTS],
    period: f32,
}

impl GpuTimer {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            period: queue.get_timestamp_period(),
            slots: std::array::from_fn(|_| Slot {
                queries: std::array::from_fn(|_| device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("pass timestamps"), ty: wgpu::QueryType::Timestamp, count: 2,
                })),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamp resolve"), size: 3 * RESOLVE_STRIDE,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamp readback"), size: BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                ready: Arc::new(AtomicU8::new(0)), frame: None, passes: [false; 3],
            }),
        }
    }

    pub(crate) fn begin(&mut self, frame_id: u64) -> Option<usize> {
        let index = self.slots.iter().position(|slot| slot.frame.is_none())?;
        let slot = &mut self.slots[index];
        slot.frame = Some(frame_id);
        slot.passes = [false; 3];
        Some(index)
    }

    pub(crate) fn writes(&mut self, index: usize, pass: usize) -> wgpu::RenderPassTimestampWrites<'_> {
        let slot = &mut self.slots[index];
        slot.passes[pass] = true;
        wgpu::RenderPassTimestampWrites {
            query_set: &slot.queries[pass],
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    pub(crate) fn submitted(&self, index: usize, queue: &wgpu::Queue) {
        let ready = Arc::clone(&self.slots[index].ready);
        queue.on_submitted_work_done(move || ready.store(1, Ordering::Release));
    }

    fn resolve(slot: &Slot, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("completed timestamp resolution"),
        });
        // Resolve only written pairs. Each resolve destination must be aligned;
        // the readback can pack those pairs contiguously.
        for (pass, active) in slot.passes.into_iter().enumerate() {
            if !active { continue; }
            let offset = pass as u64 * RESOLVE_STRIDE;
            encoder.resolve_query_set(&slot.queries[pass], 0..2, &slot.resolve, offset);
            encoder.copy_buffer_to_buffer(&slot.resolve, offset, &slot.readback, pass as u64 * 16, 16);
        }
        let ready = Arc::clone(&slot.ready);
        encoder.map_buffer_on_submit(&slot.readback, wgpu::MapMode::Read, ..,
            move |result| ready.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release));
        queue.submit(Some(encoder.finish()));
    }

    pub(crate) fn collect(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mut receive: impl FnMut(GpuCompletion)) {
        // Poll progresses callbacks without waiting for GPU completion.
        if let Err(error) = device.poll(wgpu::PollType::Poll) {
            log::warn!("GPU timing poll failed: {error}");
        }
        for slot in &mut self.slots {
            let state = slot.ready.swap(0, Ordering::Acquire);
            if state == 0 { continue; }
            if state == 1 {
                // Metal may return zero counters when resolution is committed
                // before the CPU observes sampling completion (wgpu#9414).
                // A later submission preserves that boundary without a wait.
                Self::resolve(slot, device, queue);
                continue;
            }
            let frame_id = slot.frame.take().expect("only submitted slots complete");
            let result = if state == 2 {
                let result = match slot.readback.get_mapped_range(..) {
                    Ok(mapped) => decode(bytemuck::cast_slice(&mapped), slot.passes, self.period),
                    Err(_) => Err("readback_failed"),
                };
                slot.readback.unmap();
                result
            } else { Err("map_failed") };
            receive(GpuCompletion { frame_id, result });
        }
    }
}

fn decode(values: &[u64], active: [bool; 3], period: f32) -> Result<GpuTimings, &'static str> {
    if !period.is_finite() || period <= 0.0 { return Err("invalid_timestamp_period"); }
    let mut passes = [None; 3];
    let mut unavailable = [false; 3];
    let mut earliest = u64::MAX;
    let mut latest = 0;
    let duration = |ticks: u64| Duration::try_from_secs_f64(ticks as f64 * f64::from(period) * 1e-9)
        .map_err(|_| "invalid_timestamp_duration");
    for (index, enabled) in active.into_iter().enumerate() {
        if !enabled { continue; }
        let start = values[index * 2];
        let end = values[index * 2 + 1];
        // Missing or wrapping samples never become plausible zero/short work.
        if start == 0 || end == 0 || start == u64::MAX || end == u64::MAX || end < start {
            unavailable[index] = true;
            continue;
        }
        passes[index] = Some(duration(end - start)?);
        earliest = earliest.min(start);
        latest = latest.max(end);
    }
    if !active.into_iter().any(|enabled| enabled) { return Err("no_passes"); }
    let span = if unavailable.into_iter().any(|missing| missing) { None }
        else { Some(duration(latest - earliest)?) };
    Ok(GpuTimings { passes, unavailable, span })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_and_absent_passes_are_not_added_as_gpu_work() {
        let result = decode(&[10, 30, 0, 0, 50, 90], [true, false, true], 2.0).unwrap();
        assert_eq!(result.passes, [Some(Duration::from_nanos(40)), None, Some(Duration::from_nanos(80))]);
        assert_eq!(result.span, Some(Duration::from_nanos(160)));
        let partial = decode(&[30, 10, 0, 0, 50, 90], [true, false, true], 1.0).unwrap();
        assert_eq!(partial.unavailable, [true, false, false]);
        assert_eq!(partial.passes, [None, None, Some(Duration::from_nanos(40))]);
        assert!(partial.span.is_none());
        assert!(decode(&[0; 6], [false; 3], 1.0).is_err());
        assert!(decode(&[0; 6], [true; 3], f32::NAN).is_err());
        let missing = decode(&[0; 6], [true; 3], 1.0).unwrap();
        assert_eq!(missing.unavailable, [true; 3]);
        assert_eq!(missing.passes, [None; 3]);
        assert!(missing.span.is_none());
        assert_eq!(decode(&[1, u64::MAX, 0, 0, 0, 0], [true, false, false], 1.0).unwrap().unavailable,
            [true, false, false]);
    }

    #[test]
    fn real_gpu_readbacks_keep_frame_identity_and_reuse_only_completed_slots() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU profiling tests require an adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })).expect("target GPU supports pass timestamps");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("timestamp test target"),
            size: wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("timestamp test shader"),
            source: wgpu::ShaderSource::Wgsl("@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f { let p=array(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3)); return vec4f(p[i],0,1); } @fragment fn fs()->@location(0) vec4f { return vec4f(1); }".into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("timestamp test pipeline"), layout: None,
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("timestamp depth test"), format: wgpu::TextureFormat::Depth32Float,
            size: wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("timestamp depth pipeline"), layout: None,
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: None, primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default(),
            }), multisample: Default::default(), multiview_mask: None, cache: None,
        });
        let mut timer = GpuTimer::new(&device, &queue);
        let submit = |timer: &mut GpuTimer, id| {
            let slot = timer.begin(id).expect("free timing slot");
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None,
                    }),
                    timestamp_writes: Some(timer.writes(slot, 0)), ..Default::default()
                });
                pass.set_pipeline(&depth_pipeline);
                pass.draw(0..3, 0..1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view, depth_slice: None, resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    timestamp_writes: Some(timer.writes(slot, 2)),
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.draw(0..3, 0..1);
            }
            queue.submit(Some(encoder.finish()));
            timer.submitted(slot, &queue);
        };
        let mut completed = Vec::new();
        for batch in 0..16 {
            let first = 10 + batch * GPU_TIMING_SLOTS as u64;
            for id in first..first + GPU_TIMING_SLOTS as u64 { submit(&mut timer, id); }
            assert!(timer.begin(u64::MAX).is_none(), "a full ring drops observation rather than blocking");
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            timer.collect(&device, &queue, |_| panic!("sampling completion must precede resolution"));
            assert!(timer.begin(u64::MAX).is_none(), "resolving slots cannot be reused");
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            timer.collect(&device, &queue, |result| completed.push(result));
        }
        assert_eq!(completed.iter().map(|result| result.frame_id).collect::<Vec<_>>(), (10..138).collect::<Vec<_>>());
        for result in completed {
            let times = result.result.unwrap();
            assert_eq!(times.unavailable, [false; 3], "all 128 completed frames need valid active counters");
            assert!(times.passes[0].is_some_and(|time| !time.is_zero()));
            assert!(times.passes[2].is_some_and(|time| !time.is_zero()));
            assert!(times.passes[1].is_none() && !times.unavailable[1], "inactive fire pass stays distinct from missing samples");
            let span = times.span.expect("complete frame interval");
            assert!(times.passes.into_iter().flatten().all(|pass| pass <= span));
        }
    }
}
