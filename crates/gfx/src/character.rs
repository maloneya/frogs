//! GPU resources and pipeline for one validated skinned character.

use std::num::NonZeroU64;

use arpg_core::Instance;
use wgpu::util::DeviceExt as _;

use crate::camera::CameraBinding;
use crate::cube::instance_layout;
use crate::material::{Material, MaterialLayout};
use crate::mesh::MeshUploadError;

/// One uploaded character mesh and material.
pub struct CharacterMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    material: Material,
}

impl CharacterMesh {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material_layout: &MaterialLayout,
        character: &arpg_assets::CharacterAsset,
    ) -> Result<Self, MeshUploadError> {
        let scopes = [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ]
        .map(|filter| device.push_error_scope(filter));
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("character vertices"),
            contents: bytemuck::cast_slice(character.vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("character indices"),
            contents: bytemuck::cast_slice(character.indices()),
            usage: wgpu::BufferUsages::INDEX,
        });
        let material = Material::new(
            device,
            queue,
            material_layout,
            character.base_color_texture(),
            character.base_color_factor(),
            "character base colour",
        );
        let uploaded = Self {
            vertices,
            indices,
            index_count: character.index_count() as u32,
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

/// One frame's placement and sampled pose for an uploaded character.
#[derive(Clone, Copy)]
pub struct CharacterPreview<'a> {
    pub(crate) mesh: &'a CharacterMesh,
    pub(crate) joints: &'a [arpg_assets::JointMatrix],
    pub(crate) instance: Instance,
}

impl<'a> CharacterPreview<'a> {
    /// Pairs an uploaded mesh, a sampled CPU pose and an app-owned world transform.
    #[must_use]
    pub fn new(
        mesh: &'a CharacterMesh,
        pose: &'a arpg_assets::CharacterPose,
        instance: Instance,
    ) -> Self {
        Self {
            mesh,
            joints: pose.joint_matrices(),
            instance,
        }
    }
}

pub(crate) struct CharacterPipeline {
    pipeline: wgpu::RenderPipeline,
    instance: wgpu::Buffer,
    joints: wgpu::Buffer,
    joint_bind_group: wgpu::BindGroup,
    material_layout: MaterialLayout,
}

impl CharacterPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        material_layout: MaterialLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let joint_bytes = (arpg_assets::MAX_JOINTS * size_of::<arpg_assets::JointMatrix>()) as u64;
        let joint_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("character joint palette"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(joint_bytes),
                },
                count: None,
            }],
        });
        let joints = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("character joint palette"),
            size: joint_bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let joint_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("character joint palette"),
            layout: &joint_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: joints.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("character.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("character pipeline layout"),
            bind_group_layouts: &[
                Some(camera_layout),
                Some(material_layout.raw()),
                Some(&joint_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("character pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<arpg_assets::CharacterVertex>()
                            as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3,
                            1 => Float32x3,
                            5 => Float32x2,
                            6 => Uint16x4,
                            7 => Float32x4
                        ],
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
            label: Some("character instance"),
            size: size_of::<Instance>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            instance,
            joints,
            joint_bind_group,
            material_layout,
        }
    }

    pub(crate) fn upload_character(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        character: &arpg_assets::CharacterAsset,
    ) -> Result<CharacterMesh, MeshUploadError> {
        CharacterMesh::new(device, queue, &self.material_layout, character)
    }

    pub(crate) fn upload(&self, queue: &wgpu::Queue, preview: CharacterPreview<'_>) {
        debug_assert!(!preview.joints.is_empty());
        debug_assert!(preview.joints.len() <= arpg_assets::MAX_JOINTS);
        queue.write_buffer(&self.instance, 0, bytemuck::bytes_of(&preview.instance));
        queue.write_buffer(&self.joints, 0, bytemuck::cast_slice(preview.joints));
    }

    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &CharacterMesh,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_bind_group(1, mesh.material.bind_group(), &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
    }
}
