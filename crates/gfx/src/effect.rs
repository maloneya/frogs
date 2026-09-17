//! Depth-tested translucent world triangles; no simulation vocabulary.

use glam::Vec3;

use crate::camera::CameraBinding;

/// One vertex of an unlit translucent triangle, in world metres and linear RGBA.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EffectVertex {
    position: [f32; 3],
    colour: [f32; 4],
}

impl EffectVertex {
    /// Validates a presentation vertex before it enters the GPU stream.
    #[must_use]
    pub fn new(position: Vec3, colour: glam::Vec4) -> Self {
        assert!(position.is_finite() && colour.is_finite());
        assert!((0.0..=1.0).contains(&colour.w));
        Self {
            position: position.to_array(),
            colour: colour.to_array(),
        }
    }
}

/// Fixed capacity of the transient triangle stream.
pub const MAX_EFFECT_VERTICES: usize = 8192;

pub(crate) struct EffectPipeline {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
}

impl EffectPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("effect.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect triangle layout"),
            bind_group_layouts: &[Some(camera)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("translucent world triangles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<EffectVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // Effects respect opaque geometry without occluding later transparency.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect vertices"),
            size: (MAX_EFFECT_VERTICES * size_of::<EffectVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, vertices }
    }

    pub(crate) fn upload(&self, queue: &wgpu::Queue, vertices: &[EffectVertex]) -> u32 {
        assert!(
            vertices.len().is_multiple_of(3),
            "effects must contain whole triangles"
        );
        assert!(
            vertices.len() <= MAX_EFFECT_VERTICES,
            "effect triangles exceed GPU capacity"
        );
        if !vertices.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(vertices));
        }
        vertices.len() as u32
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, camera: &CameraBinding, count: u32) {
        if count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..count, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucent_triangles_blend_respect_depth_and_clear() {
        const N: u32 = 256;
        let (device, queue) = crate::tests::headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let camera = CameraBinding::new(&device);
        let pipeline = EffectPipeline::new(
            &device,
            &camera.layout,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            crate::DEPTH_FORMAT,
        );
        let mut view_camera = crate::OrthoCamera::new(N, N);
        view_camera.zoom_by(0.125);
        camera.upload(&queue, &view_camera);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect triangle test"),
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
        let triangle = [
            EffectVertex::new(
                Vec3::new(-1.0, 0.0, -1.0),
                glam::Vec4::new(1.0, 1.0, 1.0, 0.25),
            ),
            EffectVertex::new(
                Vec3::new(1.0, 0.0, -1.0),
                glam::Vec4::new(1.0, 1.0, 1.0, 0.25),
            ),
            EffectVertex::new(
                Vec3::new(0.0, 0.0, 1.0),
                glam::Vec4::new(1.0, 1.0, 1.0, 0.25),
            ),
        ];
        for (enabled, depth_clear) in [(true, 1.0), (true, 0.0), (false, 1.0)] {
            let count = pipeline.upload(&queue, if enabled { &triangle } else { &[] });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("effect triangle test"),
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
                        // Zero depth must occlude the effect; far depth permits it.
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(depth_clear),
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
            if enabled && depth_clear == 1.0 {
                assert!(points.len() > 100, "triangle must cover visible pixels");
                let brightest = pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| p[0])
                    .max()
                    .unwrap();
                assert!(
                    (130..=140).contains(&brightest),
                    "linear alpha 0.25 must blend then encode to sRGB: {brightest}"
                );
            } else {
                assert!(
                    points.is_empty(),
                    "occluded or cleared effects must disappear"
                );
            }
        }
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}
