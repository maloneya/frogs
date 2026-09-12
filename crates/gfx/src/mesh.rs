//! GPU resources and pipeline for validated static assets.

use arpg_core::Instance;
use wgpu::util::DeviceExt as _;

use crate::camera::CameraBinding;
use crate::cube::instance_layout;

/// One uploaded static mesh.
///
/// Fields stay private so only this crate can bind its buffers or reinterpret
/// their layout. The app owns values of this type and therefore their lifetime.
pub struct MeshAsset {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    material: wgpu::BindGroup,
}

/// A GPU rejected buffers for an otherwise validated CPU mesh.
#[derive(Debug)]
pub struct MeshUploadError(String);

impl std::fmt::Display for MeshUploadError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "upload mesh: {}", self.0)
    }
}

impl std::error::Error for MeshUploadError {}

impl MeshAsset {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material_layout: &wgpu::BindGroupLayout,
        mesh: &arpg_assets::StaticMesh,
    ) -> Result<Self, MeshUploadError> {
        let scopes = [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ]
        .map(|filter| device.push_error_scope(filter));
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("asset preview vertices"),
            contents: bytemuck::cast_slice(mesh.vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("asset preview indices"),
            contents: bytemuck::cast_slice(mesh.indices()),
            usage: wgpu::BufferUsages::INDEX,
        });
        let image = mesh.base_color_texture();
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("asset preview base colour"),
                size: wgpu::Extent3d {
                    width: image.width(),
                    height: image.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Base-colour texels are sRGB; sampling decodes them to linear
                // before the shader multiplies lighting and authored factors.
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            image.rgba8(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("asset preview base colour"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let factor = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("asset preview base colour factor"),
            contents: bytemuck::cast_slice(&[mesh.base_color_factor()]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let material = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("asset preview material"),
            layout: material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: factor.as_entire_binding(),
                },
            ],
        });
        let uploaded = Self {
            vertices,
            indices,
            index_count: mesh.index_count() as u32,
            material,
        };
        let mut failure = None;
        for scope in scopes.into_iter().rev() {
            if let Some(error) = pollster::block_on(scope.pop()) {
                failure.get_or_insert_with(|| error.to_string());
            }
        }
        failure.map_or(Ok(uploaded), |error| Err(MeshUploadError(error)))
    }
}

/// One frame's placement of an uploaded mesh.
#[derive(Clone, Copy)]
pub struct MeshPreview<'a> {
    pub(crate) mesh: &'a MeshAsset,
    pub(crate) instance: Instance,
}

impl<'a> MeshPreview<'a> {
    /// Pairs a GPU asset with its app-owned world transform and tint.
    #[must_use]
    pub fn new(mesh: &'a MeshAsset, instance: Instance) -> Self {
        Self { mesh, instance }
    }
}

pub(crate) struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    instance: wgpu::Buffer,
    material_layout: wgpu::BindGroupLayout,
}

impl MeshPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("mesh.wgsl"));
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("asset preview material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("asset preview pipeline layout"),
            bind_group_layouts: &[Some(camera_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("asset preview pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<arpg_assets::Vertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 5 => Float32x2],
                    }),
                    Some(instance_layout()),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(color_format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let instance = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("asset preview instance"),
            size: size_of::<Instance>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            instance,
            material_layout,
        }
    }

    pub(crate) fn upload_mesh(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &arpg_assets::StaticMesh,
    ) -> Result<MeshAsset, MeshUploadError> {
        MeshAsset::new(device, queue, &self.material_layout, mesh)
    }

    pub(crate) fn upload(&self, queue: &wgpu::Queue, instance: Instance) {
        queue.write_buffer(&self.instance, 0, bytemuck::bytes_of(&instance));
    }

    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &MeshAsset,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_bind_group(1, &mesh.material, &[]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
    }
}
