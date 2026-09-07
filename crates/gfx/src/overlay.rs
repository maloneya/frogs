//! The overlay pipeline: screen-space quads over the finished world.
//!
//! The second pipeline in the engine, and almost the opposite of the first in
//! every choice it makes. The cube pipeline is perspective-free but still
//! camera-driven, opaque, depth-tested and back-face culled. This one has no
//! camera, blends, and is drawn strictly in submission order.
//!
//! ## Why it shares the world's render pass
//!
//! It could have been a second pass with its own colour attachment, and on a
//! tiled GPU that would cost a full store and load of the frame between the two
//! — real bandwidth, for nothing. Instead the pipeline declares a depth state
//! matching the pass it joins, and then opts out of it: never writes depth,
//! always passes the test. So the overlay is over everything, the depth buffer
//! is untouched, and the frame stays in tile memory from clear to present.
//!
//! ## Why there is no vertex buffer
//!
//! Six vertices per quad are generated from `@builtin(vertex_index)` in the
//! shader. A unit square is four numbers that never change, so uploading it
//! once per quad — or even once, as a shared mesh — would be paying a buffer
//! binding for arithmetic the vertex shader can do for free.

use crate::quad::{MAX_QUADS, Quad};
use crate::text::Font;

/// The screen's size, as the shader sees it.
///
/// A `vec4` where two floats would do, because a uniform buffer's size rounds
/// up to 16 bytes anyway and an explicitly padded struct is one that cannot
/// disagree with WGSL's layout rules.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Screen {
    size: [f32; 4],
}

const _: () = assert!(size_of::<Screen>() == 16);

/// Locations 0..2 carry `Quad`'s three `vec4`s. There is no mesh sharing the
/// slots — unlike the cube pipeline, where instance data starts at 2 — so these
/// begin at zero.
const ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];

/// Everything needed to draw a frame's worth of overlay.
pub(crate) struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    quads: wgpu::Buffer,
    screen: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl QuadPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        font: &Font,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay screen"),
            size: size_of::<Screen>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: screen.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(font.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(font.sampler()),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("overlay.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Quad>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRS,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    // Straight (non-premultiplied) alpha over. The fragment
                    // shader returns coverage in `a` and leaves `rgb` alone, so
                    // this is the blend that matches it; premultiplied would
                    // need the multiply moved into the shader and buys nothing
                    // while the overlay never renders into its own texture.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                // No culling. The corner order is fixed in the shader and an
                // overlay quad is never seen from behind, so a cull mode here
                // would be a rule with nothing to catch and one way to be
                // wrong — a winding change that blanks the whole overlay.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                // The state must exist because this pipeline joins a pass that
                // has a depth attachment. Having declared it, it then opts out
                // of both halves: `Always` puts the overlay in front of
                // everything, and not writing leaves the depth buffer as the
                // world left it.
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            quads: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay quads"),
                size: (MAX_QUADS * size_of::<Quad>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            screen,
            bind_group,
        }
    }

    /// Uploads this frame's quads and the size they are measured against,
    /// returning how many will actually be drawn.
    ///
    /// The screen size goes up every frame rather than on resize, because a
    /// uniform that is only refreshed on an event is a uniform that is stale
    /// for exactly one frame after every resize — and one stretched frame while
    /// dragging a window edge is the kind of artefact nobody can reproduce on
    /// demand. It is sixteen bytes.
    pub(crate) fn upload(
        &self,
        queue: &wgpu::Queue,
        quads: &[Quad],
        width: u32,
        height: u32,
    ) -> u32 {
        queue.write_buffer(
            &self.screen,
            0,
            bytemuck::bytes_of(&Screen { size: [width as f32, height as f32, 0.0, 0.0] }),
        );

        // The sink caps at MAX_QUADS, so this cannot truncate; the min is the
        // backstop for a caller that built a slice some other way.
        let count = quads.len().min(MAX_QUADS);
        if count > 0 {
            queue.write_buffer(&self.quads, 0, bytemuck::cast_slice(&quads[..count]));
        }
        count as u32
    }

    /// Six vertices per quad, all of them generated in the vertex shader.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, count: u32) {
        if count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.quads.slice(..));
        pass.draw(0..6, 0..count);
    }
}
