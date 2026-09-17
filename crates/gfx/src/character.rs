//! GPU resources and pipeline for one validated skinned character.

use std::num::NonZeroU64;

use std::ops::Range;

use arpg_core::{Instance, MAX_INSTANCES};
use wgpu::util::DeviceExt as _;

use crate::camera::CameraBinding;
use crate::instance::instance_layout;
use crate::material::{Material, MaterialLayout};
use crate::mesh::MeshUploadError;

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn draw_descriptions_reject_foreign_poses_before_upload() {
        let fixture = include_bytes!("../../../assets/fixtures/blender-bind-pose.glb");
        let asset = arpg_assets::import_character_glb(fixture).unwrap();
        let other = arpg_assets::import_character_glb(fixture).unwrap();
        let pose = asset.bind_pose();
        let foreign = other.bind_pose();
        assert_eq!(pose.joint_matrices().len(), foreign.joint_matrices().len());
        let (device, queue) = crate::tests::headless_device();
        let mesh = CharacterMesh::new(&device, &queue, &MaterialLayout::new(&device), &asset)
            .unwrap();
        // The mesh and pose retain their association after the CPU asset is gone.
        drop(asset);
        let instance = Instance::new(glam::Vec3::ZERO, glam::Vec3::ONE, glam::Vec3::ONE);
        let _valid = CharacterPreview::new(&mesh, &pose, instance);
        assert!(catch_unwind(AssertUnwindSafe(|| {
            CharacterPreview::new(&mesh, &foreign, instance)
        })).is_err());

        let instances = [instance];
        let valid_bucket = CharacterBucket::new(&pose, &instances);
        let _valid = CharacterHorde::new(&mesh, [valid_bucket; MAX_HORDE_POSE_BUCKETS]);
        // Every bucket is checked, even one that happens to be empty this frame.
        for index in 0..MAX_HORDE_POSE_BUCKETS {
            for placements in [&instances[..], &[][..]] {
                let mut buckets = [valid_bucket; MAX_HORDE_POSE_BUCKETS];
                buckets[index] = CharacterBucket::new(&foreign, placements);
                assert!(catch_unwind(AssertUnwindSafe(|| {
                    CharacterHorde::new(&mesh, buckets)
                })).is_err(), "foreign pose accepted in bucket {index}");
            }
        }
    }
}

/// Maximum number of shared poses one horde may draw in a frame.
///
/// This is a renderer budget rather than gameplay content. The app decides
/// which clip/phase each bucket represents, while this bound fixes GPU buffer
/// sizes at initialization.
pub const MAX_HORDE_POSE_BUCKETS: usize = 8;
const PALETTE_SLOTS: usize = 1 + MAX_HORDE_POSE_BUCKETS;

/// One uploaded character mesh and material.
pub struct CharacterMesh {
    asset_id: arpg_assets::CharacterAssetId,
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
            asset_id: character.asset_id(),
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
    mesh: &'a CharacterMesh,
    joints: &'a [arpg_assets::JointMatrix],
    instance: Instance,
}

impl<'a> CharacterPreview<'a> {
    pub(crate) fn mesh(self) -> &'a CharacterMesh {
        self.mesh
    }

    /// Pairs an uploaded mesh, a sampled CPU pose and an app-owned world transform.
    ///
    /// # Panics
    /// Panics if the pose and mesh originate from different character imports.
    #[must_use]
    pub fn new(
        mesh: &'a CharacterMesh,
        pose: &'a arpg_assets::CharacterPose,
        instance: Instance,
    ) -> Self {
        assert!(mesh.asset_id.owns(pose), "character pose belongs to a different asset than the mesh");
        Self {
            mesh,
            joints: pose.joint_matrices(),
            instance,
        }
    }
}

/// One shared joint palette and all world placements that use it.
#[derive(Clone, Copy)]
pub struct CharacterBucket<'a> {
    pose: &'a arpg_assets::CharacterPose,
    instances: &'a [Instance],
}

impl<'a> CharacterBucket<'a> {
    /// Pairs one evaluated pose with its already-grouped instances.
    #[must_use]
    pub fn new(pose: &'a arpg_assets::CharacterPose, instances: &'a [Instance]) -> Self {
        Self {
            pose,
            instances,
        }
    }
}

/// One uploaded mesh drawn through a fixed set of shared pose buckets.
#[derive(Clone, Copy)]
pub struct CharacterHorde<'a> {
    mesh: &'a CharacterMesh,
    buckets: [CharacterBucket<'a>; MAX_HORDE_POSE_BUCKETS],
}

impl<'a> CharacterHorde<'a> {
    pub(crate) fn mesh(self) -> &'a CharacterMesh {
        self.mesh
    }

    /// Creates one horde draw description without allocating or copying poses.
    ///
    /// # Panics
    /// Panics if any bucket's pose belongs to a different import than the mesh,
    /// including empty buckets. Asset selection is dynamic, so this invariant
    /// is checked at construction rather than encoded in a static Rust type.
    #[must_use]
    pub fn new(
        mesh: &'a CharacterMesh,
        buckets: [CharacterBucket<'a>; MAX_HORDE_POSE_BUCKETS],
    ) -> Self {
        assert!(buckets.iter().all(|bucket| mesh.asset_id.owns(bucket.pose)),
            "horde pose belongs to a different asset than the mesh");
        Self { mesh, buckets }
    }
}

pub(crate) struct CharacterDraws {
    pub(crate) player: Range<u32>,
    pub(crate) horde: [Range<u32>; MAX_HORDE_POSE_BUCKETS],
}

impl Default for CharacterDraws {
    fn default() -> Self {
        Self {
            player: 0..0,
            horde: std::array::from_fn(|_| 0..0),
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
        debug_assert_eq!(
            joint_bytes % u64::from(device.limits().min_uniform_buffer_offset_alignment),
            0,
            "one palette must align every dynamic uniform offset"
        );
        let joint_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("character joint palette"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(joint_bytes),
                },
                count: None,
            }],
        });
        let joints = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("character joint palettes"),
            size: joint_bytes * PALETTE_SLOTS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let joint_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("character joint palette"),
            layout: &joint_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &joints,
                    offset: 0,
                    size: NonZeroU64::new(joint_bytes),
                }),
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
            label: Some("character instances"),
            size: (MAX_INSTANCES * size_of::<Instance>()) as wgpu::BufferAddress,
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

    pub(crate) fn upload(
        &self,
        queue: &wgpu::Queue,
        player: Option<CharacterPreview<'_>>,
        horde: Option<CharacterHorde<'_>>,
    ) -> CharacterDraws {
        let instance_bytes = size_of::<Instance>() as u64;
        let palette_bytes =
            (arpg_assets::MAX_JOINTS * size_of::<arpg_assets::JointMatrix>()) as u64;
        let mut draws = CharacterDraws::default();
        let mut next_instance = 0_usize;

        if let Some(player) = player {
            debug_assert!(!player.joints.is_empty());
            debug_assert!(player.joints.len() <= arpg_assets::MAX_JOINTS);
            queue.write_buffer(&self.instance, 0, bytemuck::bytes_of(&player.instance));
            queue.write_buffer(&self.joints, 0, bytemuck::cast_slice(player.joints));
            draws.player = 0..1;
            next_instance = 1;
        }

        if let Some(horde) = horde {
            for (index, bucket) in horde.buckets.iter().enumerate() {
                let joints = bucket.pose.joint_matrices();
                debug_assert!(!joints.is_empty());
                debug_assert!(joints.len() <= arpg_assets::MAX_JOINTS);
                let count = bucket.instances.len().min(MAX_INSTANCES - next_instance);
                if count == 0 {
                    continue;
                }
                queue.write_buffer(
                    &self.instance,
                    next_instance as u64 * instance_bytes,
                    bytemuck::cast_slice(&bucket.instances[..count]),
                );
                queue.write_buffer(
                    &self.joints,
                    (index as u64 + 1) * palette_bytes,
                    bytemuck::cast_slice(joints),
                );
                draws.horde[index] = next_instance as u32..(next_instance + count) as u32;
                next_instance += count;
            }
        }
        draws
    }

    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &CharacterMesh,
        palette: usize,
        instances: Range<u32>,
    ) {
        if instances.is_empty() {
            return;
        }
        let palette_bytes =
            (arpg_assets::MAX_JOINTS * size_of::<arpg_assets::JointMatrix>()) as u32;
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_bind_group(1, mesh.material.bind_group(), &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[palette as u32 * palette_bytes]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, instances);
    }

    pub(crate) fn draw_player(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &CharacterMesh,
        instances: Range<u32>,
    ) {
        self.draw(pass, camera, mesh, 0, instances);
    }

    pub(crate) fn draw_horde(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &CharacterMesh,
        instances: &[Range<u32>; MAX_HORDE_POSE_BUCKETS],
    ) {
        for (index, instances) in instances.iter().enumerate() {
            self.draw(pass, camera, mesh, index + 1, instances.clone());
        }
    }
}
