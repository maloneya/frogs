//! GPU resources and pipeline for one validated skinned character.

use std::{num::NonZeroU64, ops::Range};

use arpg_core::{Instance, InstanceBuffer, MAX_INSTANCES};
use wgpu::util::DeviceExt as _;

use crate::camera::CameraBinding;
use crate::instance::instance_layout;
use crate::material::{Material, MaterialLayout, MaterialTexture};
use crate::mesh::MeshUploadError;

#[cfg(test)]
mod tests {
    use arpg_core::LinearRgb;
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn posed_bounds_keep_geometry_and_upload_keeps_offscreen_shadow_casters() {
        let asset=arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/basic-enemy/basic-enemy.glb")).unwrap();
        let (device,queue)=crate::tests::headless_device();
        let binding=CameraBinding::new(&device);
        let mesh=CharacterMesh::new(&device,&queue,&MaterialLayout::new(&device),&asset).unwrap();
        let mut pose=asset.bind_pose();
        for seconds in [0.0,0.125,0.25,0.375] {
            asset.sample(asset.clip_named("Run").unwrap(),seconds,&mut pose);
            let bounds=mesh.bounds(pose.joint_matrices());
            for vertex in asset.vertices() {
                let point=vertex.joints().into_iter().zip(vertex.weights()).fold(glam::Vec3::ZERO, |sum,(joint,weight)|
                    sum+pose.joint_matrices()[usize::from(joint)].matrix().transform_point3(vertex.position())*weight);
                assert!(bounds.contains_point(point),"animated vertex escaped its conservative bound");
            }
        }
        let mut pipeline=CharacterPipeline::new(&device,&binding.layout,MaterialLayout::new(&device),
            wgpu::TextureFormat::Bgra8UnormSrgb,crate::DEPTH_FORMAT);
        let placements=[Instance::new(glam::Vec3::new(10.0,0.0,0.5),glam::Vec3::ONE,LinearRgb::WHITE)];
        let empty=CharacterBucket::new(&pose,&[]);
        let mut buckets=[empty;MAX_HORDE_POSE_BUCKETS];buckets[0]=CharacterBucket::new(&pose,&placements);
        let horde=CharacterHorde::new(&mesh,buckets);
        let main=crate::visibility::Frustum::new(glam::Mat4::IDENTITY);
        let caster=crate::visibility::Frustum::new(glam::Mat4::from_translation(glam::Vec3::new(-10.0,0.0,0.0)));
        assert_eq!(pipeline.upload(&queue,None,Some(horde),&[main]).horde[0],0..0);
        assert_eq!(pipeline.upload(&queue,None,Some(horde),&[main,caster]).horde[0],0..1);
        assert_eq!(pipeline.upload(&queue,None,None,&[main]).horde[0],0..0,"removal cannot leave a stale draw");
    }

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
        let instance = Instance::new(glam::Vec3::ZERO, glam::Vec3::ONE, LinearRgb::WHITE);
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
pub const MAX_HORDE_POSE_BUCKETS: usize = 12;
const PALETTE_SLOTS: usize = 1 + MAX_HORDE_POSE_BUCKETS;

/// One uploaded character with shared geometry buffers and per-part materials.
pub struct CharacterMesh {
    asset_id: arpg_assets::CharacterAssetId,
    joint_bounds: Vec<Option<crate::visibility::Bounds>>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    parts: Vec<(Range<u32>, usize)>,
    materials: Vec<Material>,
    shadow: Option<ShadowGeometry>,
}

struct ShadowGeometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
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
        let textures: Vec<_> = character.images().iter()
            .map(|image| MaterialTexture::new(device, queue, image, "character texture"))
            .collect();
        let materials = character.materials().iter().map(|material| {
            Material::new(device, material_layout, &textures[material.image_index()],
                material.base_color_factor(), "character material")
        }).collect();
        let mut joint_bounds = vec![None; character.joint_count()];
        let shadow = character.shadow_mesh().map(|proxy| ShadowGeometry {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("coarse character caster vertices"),
                contents: bytemuck::cast_slice(proxy.vertices()), usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("coarse character caster indices"),
                contents: bytemuck::cast_slice(proxy.indices()), usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: proxy.indices().len() as u32,
        });
        // Either representation may extend beyond the other's silhouette.
        for vertex in character.vertices().iter().chain(character.shadow_mesh().into_iter().flat_map(|proxy| proxy.vertices())) {
            for (joint, weight) in vertex.joints().into_iter().zip(vertex.weights()) {
                if weight <= 0.0 { continue; }
                let bounds: &mut Option<crate::visibility::Bounds> = &mut joint_bounds[usize::from(joint)];
                if let Some(bounds) = bounds { bounds.include(vertex.position()); }
                else { *bounds=Some(crate::visibility::Bounds::point(vertex.position())); }
            }
        }
        let uploaded = Self {
            asset_id: character.asset_id(), joint_bounds, vertices, indices, materials, shadow,
            parts: character.parts().iter().map(|part| (part.indices(), part.material_index())).collect(),
        };
        let mut failure = None;
        for scope in scopes.into_iter().rev() {
            if let Some(error) = pollster::block_on(scope.pop()) {
                failure.get_or_insert_with(|| error.to_string());
            }
        }
        failure.map_or(Ok(uploaded), |error| Err(MeshUploadError(error)))
    }
    fn bounds(&self, joints: &[arpg_assets::JointMatrix]) -> crate::visibility::Bounds {
        // Every skinned vertex is a convex combination of transformed joint
        // influences. Their union bounds the full pose, including blended limbs.
        let mut combined: Option<crate::visibility::Bounds> = None;
        for (bounds, matrix) in self.joint_bounds.iter().zip(joints) {
            if let Some(bounds) = bounds {
                for point in bounds.corners() {
                    let point=matrix.matrix().transform_point3(point);
                    if let Some(bounds) = &mut combined { bounds.include(point); }
                    else { combined=Some(crate::visibility::Bounds::point(point)); }
                }
            }
        }
        combined.expect("validated character contains weighted geometry")
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

/// One uploaded character drawn through a fixed set of shared pose buckets.
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
    depth_pipeline: wgpu::RenderPipeline,
    shadows: wgpu::RenderPipeline,
    instance: wgpu::Buffer,
    staging: InstanceBuffer,
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
        let shader = crate::lighting::shader(device, include_str!("character.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("character pipeline layout"),
            bind_group_layouts: &[
                Some(camera_layout),
                Some(material_layout.raw()),
                Some(&joint_layout),
            ],
            immediate_size: 0,
        });
        let buffers = [
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
                ];
        let depth_pipeline = crate::shadow::pipeline(device, &shader, &[material_layout.raw(), &joint_layout], &buffers);
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("character pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &buffers,
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
        let shadow_shader = crate::lighting::shader(device, include_str!("shadow.wgsl"));
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("contact shadow layout"), bind_group_layouts: &[Some(camera_layout)], immediate_size: 0,
        });
        let shadows = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("soft character grounding"), layout: Some(&shadow_layout),
            vertex: wgpu::VertexState { module: &shadow_shader, entry_point: Some("vs_main"),
                compilation_options: Default::default(), buffers: &[Some(instance_layout())] },
            fragment: Some(wgpu::FragmentState { module: &shadow_shader, entry_point: Some("fs_main"),
                compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState {
                    format: color_format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL,
                })] }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: depth_format,
                depth_write_enabled: Some(false), depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview_mask: None, cache: None,
        });
        let instance = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("character instances"),
            size: (MAX_INSTANCES * size_of::<Instance>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            depth_pipeline,
            shadows,
            instance,
            staging: InstanceBuffer::default(),
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
        &mut self,
        queue: &wgpu::Queue,
        player: Option<CharacterPreview<'_>>,
        horde: Option<CharacterHorde<'_>>,
        views: &[crate::visibility::Frustum],
    ) -> CharacterDraws {
        assert!(!views.is_empty(), "character upload requires a render view");
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
                let bounds=horde.mesh.bounds(joints);
                let mut sink = self.staging.sink();
                for instance in bucket.instances.iter().copied()
                    .filter(|&instance| bounds.visible(instance,views)).take(MAX_INSTANCES-next_instance) {
                    sink.push(instance);
                }
                let count=self.staging.as_slice().len();
                if count == 0 { continue; }
                queue.write_buffer(&self.instance,next_instance as u64*instance_bytes,bytemuck::cast_slice(self.staging.as_slice()));
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

    pub(crate) fn draw_shadows(&self, pass: &mut wgpu::RenderPass<'_>, camera: &CameraBinding, draws: &CharacterDraws) {
        let end = draws.horde.iter().map(|range| range.end).max().unwrap_or(0).max(draws.player.end);
        if end == 0 { return; }
        pass.set_pipeline(&self.shadows);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance.slice(..));
        pass.draw(0..6, 0..end);
    }

    pub(crate) fn draw_depth(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup,
        player: Option<CharacterPreview<'_>>, horde: Option<CharacterHorde<'_>>, draws: &CharacterDraws) {
        if let Some(player) = player { self.draw_caster(pass, camera, player.mesh(), 0, draws.player.clone()); }
        if let Some(horde) = horde {
            for (i, instances) in draws.horde.iter().enumerate() { self.draw_caster(pass, camera, horde.mesh(), i+1, instances.clone()); }
        }
    }

    fn draw_caster(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup,
        mesh: &CharacterMesh, palette: usize, instances: Range<u32>) {
        let Some(shadow) = &mesh.shadow else {
            self.draw(pass, (camera, &self.depth_pipeline), mesh, palette, instances);
            return;
        };
        if instances.is_empty() { return; }
        let palette_bytes = (arpg_assets::MAX_JOINTS * size_of::<arpg_assets::JointMatrix>()) as u32;
        pass.set_pipeline(&self.depth_pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, mesh.materials[0].bind_group(), &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[palette as u32 * palette_bytes]);
        pass.set_vertex_buffer(0, shadow.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        pass.set_index_buffer(shadow.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..shadow.index_count, 0, instances);
    }

    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        view: (&wgpu::BindGroup, &wgpu::RenderPipeline),
        mesh: &CharacterMesh,
        palette: usize,
        instances: Range<u32>,
    ) {
        if instances.is_empty() {
            return;
        }
        let palette_bytes =
            (arpg_assets::MAX_JOINTS * size_of::<arpg_assets::JointMatrix>()) as u32;
        pass.set_pipeline(view.1);
        pass.set_bind_group(0, view.0, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[palette as u32 * palette_bytes]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        for (indices, material) in &mesh.parts {
            pass.set_bind_group(1, mesh.materials[*material].bind_group(), &[]);
            pass.draw_indexed(indices.clone(), 0, instances.clone());
        }
    }

    pub(crate) fn draw_player(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &CharacterMesh,
        instances: Range<u32>,
    ) {
        self.draw(pass, (&camera.bind_group, &self.pipeline), mesh, 0, instances);
    }

    pub(crate) fn draw_horde(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &CharacterMesh,
        instances: &[Range<u32>; MAX_HORDE_POSE_BUCKETS],
    ) {
        for (index, instances) in instances.iter().enumerate() {
            self.draw(pass, (&camera.bind_group, &self.pipeline), mesh, index + 1, instances.clone());
        }
    }
}
