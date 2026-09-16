//! GPU resources and pipeline for validated static assets.

use arpg_core::{Instance, MAX_INSTANCES};
use wgpu::util::DeviceExt as _;

use crate::camera::CameraBinding;
use crate::instance::instance_layout;
use crate::material::{Material, MaterialLayout};

/// One uploaded static mesh.
///
/// Fields stay private so only this crate can bind its buffers or reinterpret
/// their layout. The app owns values of this type and therefore their lifetime.
pub struct MeshAsset {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    material: Material,
}

/// A GPU rejected buffers for an otherwise validated CPU mesh.
#[derive(Debug)]
pub struct MeshUploadError(pub(crate) String);

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
        material_layout: &MaterialLayout,
        mesh: &arpg_assets::StaticMesh,
    ) -> Result<Self, MeshUploadError> {
        let scopes = [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ]
        .map(|filter| device.push_error_scope(filter));
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("static mesh vertices"),
            contents: bytemuck::cast_slice(mesh.vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("static mesh indices"),
            contents: bytemuck::cast_slice(mesh.indices()),
            usage: wgpu::BufferUsages::INDEX,
        });
        let material = Material::new(
            device,
            queue,
            material_layout,
            mesh.base_color_texture(),
            mesh.base_color_factor(),
            "static mesh base colour",
        );
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

/// One uploaded mesh and all its placements for this frame.
///
/// Each nonempty batch becomes one instanced draw. Across a frame, batches
/// must contain at most [`MAX_INSTANCES`] placements; upload validates this
/// before writing anything, so no geometry is silently truncated.
#[derive(Clone, Copy)]
pub struct MeshBatch<'a> {
    pub(crate) mesh: &'a MeshAsset,
    pub(crate) instances: &'a [Instance],
}

impl<'a> MeshBatch<'a> {
    /// Borrows validated GPU geometry and presentation-owned placements.
    #[must_use]
    pub fn new(mesh: &'a MeshAsset, instances: &'a [Instance]) -> Self {
        Self { mesh, instances }
    }
}

pub(crate) struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    instance: wgpu::Buffer,
    material_layout: MaterialLayout,
}

impl MeshPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        material_layout: MaterialLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("mesh.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("static mesh pipeline layout"),
            bind_group_layouts: &[Some(camera_layout), Some(material_layout.raw())],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("static mesh pipeline"),
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
            label: Some("static mesh instance"),
            size: (MAX_INSTANCES * size_of::<Instance>()) as wgpu::BufferAddress,
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

    pub(crate) fn upload(&self, queue: &wgpu::Queue, batches: &[MeshBatch<'_>]) {
        let count = batches.iter().try_fold(0_usize, |count, batch| {
            count.checked_add(batch.instances.len())
        }).expect("static mesh instance count overflow");
        assert!(count <= MAX_INSTANCES, "static mesh batches exceed instance capacity");
        let mut offset = 0;
        for batch in batches {
            if !batch.instances.is_empty() {
                queue.write_buffer(
                    &self.instance,
                    (offset * size_of::<Instance>()) as wgpu::BufferAddress,
                    bytemuck::cast_slice(batch.instances),
                );
                offset += batch.instances.len();
            }
        }
    }

    pub(crate) fn draw_batches(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        batches: &[MeshBatch<'_>],
    ) {
        let mut first = 0;
        for batch in batches {
            let end = first + batch.instances.len() as u32;
            self.draw(pass, camera, batch.mesh, first..end);
            first = end;
        }
    }

    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &MeshAsset,
        instances: std::ops::Range<u32>,
    ) {
        if instances.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_bind_group(1, mesh.material.bind_group(), &[]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, instances);
    }
}
