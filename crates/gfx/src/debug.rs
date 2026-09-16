//! Unlit, always-visible diagnostic disc outlines, independent of asset geometry.

use arpg_core::MAX_INSTANCES;
use glam::Vec3;
use wgpu::util::DeviceExt as _;

use crate::camera::CameraBinding;

/// A ground-plane disc outline. Colours are linear RGB; positions and radii use metres.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DebugDisc {
    centre_radius: [f32; 4],
    colour: [f32; 4],
}

impl DebugDisc {
    /// Validates geometry at the only construction boundary.
    #[must_use]
    pub fn new(centre: Vec3, radius: f32, colour: Vec3) -> Self {
        assert!(centre.is_finite() && radius.is_finite() && radius > 0.0);
        assert!(colour.is_finite());
        Self {
            centre_radius: [centre.x, centre.y, centre.z, radius],
            colour: [colour.x, colour.y, colour.z, 1.0],
        }
    }

    /// World-space centre of the displayed outline.
    #[must_use]
    pub fn centre(self) -> Vec3 {
        Vec3::from_slice(&self.centre_radius[..3])
    }

    /// Radius of the displayed outline, in metres.
    #[must_use]
    pub fn radius(self) -> f32 {
        self.centre_radius[3]
    }
}

const SEGMENTS: u32 = 48;

pub(crate) struct DebugPipeline {
    pipeline: wgpu::RenderPipeline,
    circle: wgpu::Buffer,
    discs: wgpu::Buffer,
}

impl DebugPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("debug.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("debug disc layout"),
            bind_group_layouts: &[Some(camera)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("debug disc outlines"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<DebugDisc>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![1 => Float32x4, 2 => Float32x4],
                    }),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            // Diagnostics remain visible through art, but cannot affect its depth.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let points: Vec<[f32; 2]> = (0..SEGMENTS)
            .flat_map(|segment| {
                [segment, (segment + 1) % SEGMENTS].map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / SEGMENTS as f32;
                    [angle.cos(), angle.sin()]
                })
            })
            .collect();
        let circle = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("unit debug circle"),
            contents: bytemuck::cast_slice(&points),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let discs = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("debug disc instances"),
            size: (MAX_INSTANCES * size_of::<DebugDisc>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            circle,
            discs,
        }
    }

    pub(crate) fn upload(&self, queue: &wgpu::Queue, discs: &[DebugDisc]) -> u32 {
        assert!(
            discs.len() <= MAX_INSTANCES,
            "debug discs exceed GPU capacity"
        );
        if !discs.is_empty() {
            queue.write_buffer(&self.discs, 0, bytemuck::cast_slice(discs));
        }
        discs.len() as u32
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, camera: &CameraBinding, count: u32) {
        if count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_vertex_buffer(0, self.circle.slice(..));
        pass.set_vertex_buffer(1, self.discs.slice(..));
        pass.draw(0..SEGMENTS * 2, 0..count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_outlines_ignore_occlusion_and_disappear_when_disabled() {
        const N: u32 = 256;
        let (device, queue) = crate::tests::headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let camera = CameraBinding::new(&device);
        let pipeline = DebugPipeline::new(
            &device,
            &camera.layout,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            crate::DEPTH_FORMAT,
        );
        let mut view_camera = crate::OrthoCamera::new(N, N);
        view_camera.zoom_by(0.125);
        camera.upload(&queue, &view_camera);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("debug outline test"),
            size: wgpu::Extent3d {
                width: N,
                height: N,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let depth = crate::create_depth(&device, N, N);
        let disc = [DebugDisc::new(Vec3::ZERO, 1.0, Vec3::ONE)];
        for enabled in [true, false] {
            let count = pipeline.upload(&queue, if enabled { &disc } else { &[] });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("debug outline test"),
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
                        // Nearest possible depth would hide every line if the
                        // diagnostic pipeline accidentally used normal depth testing.
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pipeline.draw(&mut pass, &camera, count);
            }
            let readback = crate::capture::Readback::new(&device, N, N);
            readback.record(&mut encoder, &target);
            queue.submit(Some(encoder.finish()));
            let pixels = readback.to_rgba(&device).unwrap();
            let points: Vec<_> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, p)| p[0] > 128)
                .map(|(i, _)| (i as u32 % N, i as u32 / N))
                .collect();
            if enabled {
                assert!(
                    points.len() > 100 && points.len() < 1000,
                    "expect thin lines, not a filled disc: {}",
                    points.len()
                );
                let width = points.iter().map(|p| p.0).max().unwrap()
                    - points.iter().map(|p| p.0).min().unwrap();
                let height = points.iter().map(|p| p.1).max().unwrap()
                    - points.iter().map(|p| p.1).min().unwrap();
                assert!(
                    width > 100 && height > 50 && width > height,
                    "disc must lie on XZ: {width}x{height}"
                );
                assert_eq!(
                    pixels[((N / 2 * N + N / 2) * 4) as usize],
                    0,
                    "disc interior stays empty"
                );
            } else {
                assert!(
                    points.is_empty(),
                    "stale GPU instances must not draw after disable"
                );
            }
        }
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}
