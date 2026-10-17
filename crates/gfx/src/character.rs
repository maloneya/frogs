//! GPU resources and pipeline for one validated skinned character.

use std::{num::NonZeroU64, ops::Range};

mod posed;

use arpg_core::{Instance, MAX_INSTANCES};
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
        let mut asset=arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/wolf/wolf.glb")).unwrap();
        asset.attach_shadow(arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/wolf/wolf.shadow.glb")).unwrap()).unwrap();
        let (device,queue)=crate::tests::headless_device();
        let binding=CameraBinding::new(&device);
        let mesh=CharacterMesh::new(&device,&queue,&MaterialLayout::new(&device),&asset).unwrap();
        let mut pose=asset.bind_pose();
        for seconds in [0.0,0.125,0.25,0.375] {
            asset.sample(asset.clip_named("Run").unwrap(),seconds,&mut pose);
            let bounds=mesh.bounds(pose.joint_matrices());
            for vertex in asset.vertices().iter().chain(asset.shadow_mesh().unwrap().vertices()) {
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
        assert_eq!(pipeline.upload(&device, &queue, None, Some(horde), &[Some(main), None, None]).end, 0);
        let draws = pipeline.upload(&device, &queue, None, Some(horde), &[Some(main), Some(caster), None]);
        assert_eq!(draws.end, 1);
        assert_eq!(draws.groups[2].horde[0], 0..1);
        assert!(draws.groups.iter().enumerate().filter(|(mask, _)| mask & 1 != 0)
            .all(|(_, group)| group.horde[0].is_empty()), "offscreen caster must not be submitted to main");
        let draws = pipeline.upload(&device, &queue, None, None, &[Some(main), None, None]);
        assert_eq!(draws.end, 0);
        assert!(pipeline.posed_horde.is_none(), "removed horde releases its posed resources");
        assert!(draws.groups.iter().all(|group| group.player.is_empty() && group.horde.iter().all(Range::is_empty)),
            "removal cannot leave a stale draw");
        let player = CharacterPreview::new(&mesh, &pose, placements[0]);
        let draws = pipeline.upload(&device, &queue, Some(player), None, &[Some(main), None, Some(caster)]);
        assert_eq!(draws.groups[5].player, 0..1, "player keeps its main draw and the fire keeps its own slot");
    }

    #[test]
    fn visibility_groups_pack_each_intersection_once_and_reset_with_a_bounded_capacity() {
        use glam::{Mat4, Vec3};
        use crate::visibility::{Bounds, Frustum};
        let views = [Some(Frustum::new(Mat4::IDENTITY)),
            Some(Frustum::new(Mat4::from_translation(Vec3::new(-2.0, 0.0, 0.0)))),
            Some(Frustum::new(Mat4::from_translation(Vec3::new(-1.0, -2.0, 0.0))))];
        let mut bounds = Bounds::point(Vec3::splat(-0.05));
        bounds.include(Vec3::splat(0.05));
        let placements = [(1, 0.0, 0.0), (2, 2.5, 0.0), (3, 1.0, 0.0),
            (4, 1.0, 2.5), (5, 0.5, 1.0), (6, 2.0, 1.0), (7, 1.0, 1.0), (0, 6.0, 6.0)]
            .map(|(mask, x, y)| (mask, Instance::new(Vec3::new(x, y, 0.5), Vec3::ONE, LinearRgb::WHITE)));
        for (mask, instance) in placements { assert_eq!(visibility_mask(bounds, instance, &views), mask); }
        let instances: Vec<_> = placements.into_iter().rev().flat_map(|(_, instance)| [instance; 2]).collect();
        let mut staging = CharacterInstances::default();
        let ranges = staging.pack(&instances, bounds, &views, MAX_INSTANCES);
        assert_eq!(staging.instances.len(), 14, "intersections must not duplicate an upload");
        assert!(ranges[0].is_empty());
        for (mask, range) in ranges.iter().enumerate().skip(1) {
            assert_eq!(range.len(), 2);
            for instance in &staging.instances[range.start as usize..range.end as usize] {
                assert_eq!(visibility_mask(bounds, *instance, &views), mask);
            }
        }
        let ranges = staging.pack(&instances, bounds, &views, 3);
        assert_eq!(staging.instances.len(), 3);
        assert_eq!(ranges.iter().map(Range::len).sum::<usize>(), 3);
        assert!(staging.pack(&[], bounds, &views, MAX_INSTANCES).iter().all(Range::is_empty));
        assert!(staging.instances.is_empty());
        assert!(staging.pack(&instances, bounds, &views, 0).iter().all(Range::is_empty));
    }

    #[test]
    fn horde_resources_follow_asset_identity_and_every_live_pose_is_uploaded_again() {
        let bytes = include_bytes!("../../../assets/characters/wolf/wolf.glb");
        let asset = arpg_assets::import_character_glb(bytes).unwrap();
        let other = arpg_assets::import_character_glb(bytes).unwrap();
        let (device, queue) = crate::tests::headless_device();
        let camera = CameraBinding::new(&device);
        let mut pipeline = CharacterPipeline::new(&device, &camera.layout, MaterialLayout::new(&device),
            wgpu::TextureFormat::Bgra8UnormSrgb, crate::DEPTH_FORMAT);
        let mesh = pipeline.upload_character(&device, &queue, &asset).unwrap();
        let replacement = pipeline.upload_character(&device, &queue, &other).unwrap();
        let instances = [Instance::new(glam::Vec3::ZERO, glam::Vec3::ONE, LinearRgb::WHITE)];
        let view = [Some(crate::visibility::Frustum::new(glam::Mat4::IDENTITY)), None, None];
        let mut pose = asset.bind_pose();
        let later = other.bind_pose();
        for seconds in [0.0, 0.25, 0.0] {
            asset.sample(asset.clip_named("Run").unwrap(), seconds, &mut pose);
            let empty = CharacterBucket::new(&pose, &[]);
            let mut buckets = [empty; MAX_HORDE_POSE_BUCKETS];
            buckets[0] = CharacterBucket::new(&pose, &instances);
            buckets[MAX_HORDE_POSE_BUCKETS - 1] = CharacterBucket::new(&pose, &instances);
            let draws = pipeline.upload(&device, &queue, None, Some(CharacterHorde::new(&mesh, buckets)), &view);
            assert_eq!(draws.end, 2);
            assert!(pipeline.posed_horde.as_ref().unwrap().fits(&mesh, &pose));
            assert!(!pipeline.posed_horde.as_ref().unwrap().fits(&replacement, &later));
        }
        let buckets = [CharacterBucket::new(&later, &instances); MAX_HORDE_POSE_BUCKETS];
        let draws = pipeline.upload(&device, &queue, None,
            Some(CharacterHorde::new(&replacement, buckets)), &view);
        assert_eq!(draws.end, MAX_HORDE_POSE_BUCKETS as u32);
        assert!(pipeline.posed_horde.as_ref().unwrap().fits(&replacement, &later));
        assert!(!pipeline.posed_horde.as_ref().unwrap().fits(&mesh, &pose));
        assert_eq!(pipeline.upload(&device, &queue, None, None, &view).end, 0);
        assert!(pipeline.posed_horde.is_none());
    }

    #[test]
    fn same_import_caster_changes_reallocate_pose_storage_before_reusing_it() {
        let mut asset = arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/wolf/wolf.glb")).unwrap();
        let (device, queue) = crate::tests::headless_device();
        let camera = CameraBinding::new(&device);
        let mut pipeline = CharacterPipeline::new(&device, &camera.layout, MaterialLayout::new(&device),
            wgpu::TextureFormat::Bgra8UnormSrgb, crate::DEPTH_FORMAT);
        let original = pipeline.upload_character(&device, &queue, &asset).unwrap();
        let pose = asset.bind_pose();
        asset.attach_shadow(arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/wolf/wolf.shadow.glb")).unwrap()).unwrap();
        let with_proxy = pipeline.upload_character(&device, &queue, &asset).unwrap();
        assert_ne!(original.source.len(), with_proxy.shadow.as_ref().unwrap().source.len());
        let instances = [Instance::new(glam::Vec3::ZERO, glam::Vec3::ONE, LinearRgb::WHITE)];
        let empty = CharacterBucket::new(&pose, &[]);
        let mut buckets = [empty; MAX_HORDE_POSE_BUCKETS];
        buckets[0] = CharacterBucket::new(&pose, &instances);
        buckets[MAX_HORDE_POSE_BUCKETS - 1] = CharacterBucket::new(&pose, &instances);
        let views = [Some(crate::visibility::Frustum::new(glam::Mat4::IDENTITY)), None, None];
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        for (mesh, other) in [(&with_proxy, &original), (&original, &with_proxy),
            (&with_proxy, &original), (&original, &with_proxy)] {
            let draws = pipeline.upload(&device, &queue, None, Some(CharacterHorde::new(mesh, buckets)), &views);
            assert_eq!(draws.end, 2);
            let prepared = pipeline.posed_horde.as_ref().unwrap();
            assert!(prepared.fits(mesh, &pose));
            assert!(!prepared.fits(other, &pose), "import identity alone cannot prove buffer suitability");
            // The final occupied bucket exposes a too-small depth allocation,
            // rather than merely overwriting the next bucket's storage.
            queue.submit([]);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        assert!(pollster::block_on(scope.pop()).is_none());
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
// Four collapse phases join the existing gait, action, recoil and held-death poses.
pub const MAX_HORDE_POSE_BUCKETS: usize = 20;

/// One uploaded character with shared geometry buffers and per-part materials.
pub struct CharacterMesh {
    asset_id: arpg_assets::CharacterAssetId,
    joint_bounds: Vec<Option<crate::visibility::Bounds>>,
    source: Box<[arpg_assets::CharacterVertex]>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    parts: Vec<(Range<u32>, usize)>,
    materials: Vec<Material>,
    shadow: Option<ShadowGeometry>,
}

struct ShadowGeometry {
    source: Box<[arpg_assets::CharacterVertex]>,
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
            source: proxy.vertices().into(),
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
            asset_id: character.asset_id(), joint_bounds, source: character.vertices().into(), vertices, indices, materials, shadow,
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

const CHARACTER_VIEWS: usize = 3;
const VISIBILITY_GROUPS: usize = 1 << CHARACTER_VIEWS;

type CharacterViews = [Option<crate::visibility::Frustum>; CHARACTER_VIEWS];

#[derive(Default)]
struct CharacterGroup {
    player: Range<u32>,
    horde: [Range<u32>; MAX_HORDE_POSE_BUCKETS],
}

pub(crate) struct CharacterDraws {
    groups: [CharacterGroup; VISIBILITY_GROUPS],
    end: u32,
}

impl Default for CharacterDraws {
    fn default() -> Self {
        Self { groups: std::array::from_fn(|_| CharacterGroup::default()), end: 0 }
    }
}

#[cfg(test)]
impl CharacterDraws {
    pub(crate) fn player_from_horde(&mut self, bucket: usize) {
        for group in &mut self.groups { group.player = group.horde[bucket].clone(); }
    }

    pub(crate) fn union_submissions(&mut self) {
        let mut union = CharacterGroup::default();
        let extend = |into: &mut Range<u32>, range: &Range<u32>| {
            if range.is_empty() { return; }
            if Range::is_empty(into) { *into = range.clone(); }
            else { into.start = into.start.min(range.start); into.end = into.end.max(range.end); }
        };
        for group in &self.groups {
            extend(&mut union.player, &group.player);
            for (into, range) in union.horde.iter_mut().zip(&group.horde) { extend(into, range); }
        }
        self.groups = std::array::from_fn(|_| CharacterGroup::default());
        self.groups[VISIBILITY_GROUPS - 1] = union;
    }
}

fn visibility_mask(bounds: crate::visibility::Bounds, instance: Instance, views: &CharacterViews) -> usize {
    let (centre, radius) = bounds.world_sphere(instance);
    views.iter().enumerate().fold(0, |mask, (view, frustum)| {
        mask | (usize::from(frustum.is_some_and(|frustum| frustum.contains_sphere(centre, radius))) << view)
    })
}

struct CharacterInstances {
    instances: Vec<Instance>,
    masks: Vec<u8>,
}

impl Default for CharacterInstances {
    fn default() -> Self {
        Self { instances: Vec::with_capacity(MAX_INSTANCES), masks: Vec::with_capacity(MAX_INSTANCES) }
    }
}

impl CharacterInstances {
    fn pack(&mut self, instances: &[Instance], bounds: crate::visibility::Bounds,
        views: &CharacterViews, limit: usize) -> [Range<u32>; VISIBILITY_GROUPS] {
        self.instances.clear();
        self.masks.clear();
        let mut counts = [0; VISIBILITY_GROUPS];
        for &instance in instances {
            if self.instances.len() == limit { break; }
            let mask = visibility_mask(bounds, instance, views);
            if mask == 0 { continue; }
            self.instances.push(instance);
            self.masks.push(mask as u8);
            counts[mask] += 1;
        }
        let mut end = 0;
        let ranges = counts.map(|count| { let start = end; end += count; start as u32..end as u32 });
        let mut next = ranges.clone().map(|range| range.start as usize);
        // Counting partition keeps one copy of each placement. Each pass can
        // skip entire irrelevant groups without growing the instance budget.
        for (group, range) in ranges.iter().enumerate() {
            while next[group] < range.end as usize {
                let index = next[group];
                let actual = usize::from(self.masks[index]);
                if actual == group { next[group] += 1; }
                else {
                    self.instances.swap(index, next[actual]);
                    self.masks.swap(index, next[actual]);
                    next[actual] += 1;
                }
            }
        }
        ranges
    }
}

pub(crate) struct CharacterPipeline {
    pipeline: wgpu::RenderPipeline,
    depth_pipeline: wgpu::RenderPipeline,
    posed_pipeline: wgpu::RenderPipeline,
    posed_depth: wgpu::RenderPipeline,
    posed_horde: Option<posed::Horde>,
    shadows: wgpu::RenderPipeline,
    instance: wgpu::Buffer,
    staging: CharacterInstances,
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
            label: Some("character joint palettes"),
            size: joint_bytes,
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
        let depth_pipeline = crate::shadow::pipeline(device, &shader, &[material_layout.raw(), &joint_layout], &buffers, "vs_main");
        let posed_buffers = [Some(posed::visible_layout()), Some(instance_layout())];
        let posed_depth_buffers = [Some(posed::depth_layout()), Some(instance_layout())];
        let posed_depth = crate::shadow::pipeline(device, &shader, &[], &posed_depth_buffers, "vs_posed_depth");
        let posed_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("posed horde layout"),
            bind_group_layouts: &[Some(camera_layout), Some(material_layout.raw())],
            immediate_size: 0,
        });
        let covered = wgpu::StencilFaceState { pass_op: wgpu::StencilOperation::Zero, ..Default::default() };
        let make_pipeline = |entry, layout: &wgpu::PipelineLayout, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                buffers,
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
                stencil: wgpu::StencilState { front: covered, back: covered, read_mask: 1, write_mask: 1 },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let pipeline = make_pipeline("vs_main", &layout, &buffers);
        let posed_pipeline = make_pipeline("vs_posed", &posed_layout, &posed_buffers);
        let shadow_shader = crate::lighting::shader(device, include_str!("shadow.wgsl"));
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("contact shadow layout"), bind_group_layouts: &[Some(camera_layout)], immediate_size: 0,
        });
        // The previous order let opaque characters overwrite grounding, even
        // feet below its 8 mm plane. Clear=1, character depth-pass=0 preserves
        // that exclusion after moving the blend behind all opaque draws.
        let uncovered = wgpu::StencilFaceState { compare: wgpu::CompareFunction::NotEqual, ..Default::default() };
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
                stencil: wgpu::StencilState { front: uncovered, back: uncovered, read_mask: 1, write_mask: 0 }, bias: Default::default() }),
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
            posed_pipeline, posed_depth, posed_horde: None,
            shadows,
            instance,
            staging: CharacterInstances::default(),
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
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        player: Option<CharacterPreview<'_>>,
        horde: Option<CharacterHorde<'_>>,
        views: &CharacterViews,
    ) -> CharacterDraws {
        assert!(views[0].is_some(), "character upload requires the main view");
        let instance_bytes = size_of::<Instance>() as u64;
        let mut draws = CharacterDraws::default();
        let mut next_instance = 0_usize;

        if let Some(player) = player {
            debug_assert!(!player.joints.is_empty());
            debug_assert!(player.joints.len() <= arpg_assets::MAX_JOINTS);
            queue.write_buffer(&self.instance, 0, bytemuck::bytes_of(&player.instance));
            queue.write_buffer(&self.joints, 0, bytemuck::cast_slice(player.joints));
            let mask = visibility_mask(player.mesh.bounds(player.joints), player.instance, views) | 1;
            draws.groups[mask].player = 0..1;
            next_instance = 1;
        }

        if let Some(horde) = horde {
            if !self.posed_horde.as_ref().is_some_and(|prepared| prepared.fits(horde.mesh, horde.buckets[0].pose)) {
                self.posed_horde = Some(posed::Horde::new(device, horde.mesh));
            }
            for (index, bucket) in horde.buckets.iter().enumerate() {
                let joints = bucket.pose.joint_matrices();
                debug_assert!(!joints.is_empty());
                debug_assert!(joints.len() <= arpg_assets::MAX_JOINTS);
                let ranges = self.staging.pack(bucket.instances, horde.mesh.bounds(joints),
                    views, MAX_INSTANCES - next_instance);
                let count = self.staging.instances.len();
                if count == 0 { continue; }
                queue.write_buffer(&self.instance, next_instance as u64 * instance_bytes,
                    bytemuck::cast_slice(&self.staging.instances));
                self.posed_horde.as_mut().expect("horde resources prepared").upload(
                    queue, horde.mesh, bucket.pose, index);
                for (group, range) in draws.groups.iter_mut().zip(ranges) {
                    group.horde[index] = range.start + next_instance as u32..range.end + next_instance as u32;
                }
                next_instance += count;
            }
        }
        if horde.is_none() { self.posed_horde = None; }
        draws.end = next_instance as u32;
        draws
    }

    pub(crate) fn draw_shadows(&self, pass: &mut wgpu::RenderPass<'_>, camera: &CameraBinding, draws: &CharacterDraws) {
        if draws.end == 0 { return; }
        pass.set_pipeline(&self.shadows);
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance.slice(..));
        // Grounding footprints can remain visible when the skinned mesh is
        // offscreen or airborne, so retain the original union of placements.
        pass.draw(0..6, 0..draws.end);
    }

    pub(crate) fn draw_depth(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup,
        player: Option<CharacterPreview<'_>>, horde: Option<CharacterHorde<'_>>, draws: &CharacterDraws, layer: usize) {
        let visible = |mask: usize| mask & (1 << (layer + 1)) != 0;
        if let Some(player) = player {
            for (_, group) in draws.groups.iter().enumerate().filter(|(mask, _)| visible(*mask)) {
                self.draw_caster(pass, camera, player.mesh(), group.player.clone());
            }
        }
        if let Some(horde) = horde {
            for index in 0..MAX_HORDE_POSE_BUCKETS {
                for (_, group) in draws.groups.iter().enumerate().filter(|(mask, _)| visible(*mask)) {
                    self.draw_posed_depth(pass, camera, horde.mesh(), index, group.horde[index].clone());
                }
            }
        }
    }

    fn draw_posed_depth(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup,
        mesh: &CharacterMesh, bucket: usize, instances: Range<u32>) {
        if instances.is_empty() { return; }
        let posed = self.posed_horde.as_ref().expect("horde resources prepared");
        pass.set_pipeline(&self.posed_depth);
        pass.set_bind_group(0, camera, &[]);
        pass.set_vertex_buffer(0, posed.depth(bucket));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        if let Some(shadow) = &mesh.shadow {
            pass.set_index_buffer(shadow.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..shadow.index_count, 0, instances);
        } else {
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            for (indices, _) in &mesh.parts { pass.draw_indexed(indices.clone(), 0, instances.clone()); }
        }
    }

    fn draw_posed(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup,
        mesh: &CharacterMesh, bucket: usize, instances: Range<u32>) {
        if instances.is_empty() { return; }
        let posed = self.posed_horde.as_ref().expect("horde resources prepared");
        pass.set_pipeline(&self.posed_pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_vertex_buffer(0, posed.visible(bucket));
        pass.set_vertex_buffer(1, self.instance.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        for (indices, material) in &mesh.parts {
            pass.set_bind_group(1, mesh.materials[*material].bind_group(), &[]);
            pass.draw_indexed(indices.clone(), 0, instances.clone());
        }
    }

    fn draw_caster(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup,
        mesh: &CharacterMesh, instances: Range<u32>) {
        let Some(shadow) = &mesh.shadow else {
            self.draw(pass, (camera, &self.depth_pipeline), mesh, instances);
            return;
        };
        if instances.is_empty() { return; }
        pass.set_pipeline(&self.depth_pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, mesh.materials[0].bind_group(), &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
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
        instances: Range<u32>,
    ) {
        if instances.is_empty() {
            return;
        }
        pass.set_pipeline(view.1);
        pass.set_bind_group(0, view.0, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
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
        draws: &CharacterDraws,
    ) {
        for (_, group) in draws.groups.iter().enumerate().filter(|(mask, _)| mask & 1 != 0) {
            self.draw(pass, (&camera.bind_group, &self.pipeline), mesh, group.player.clone());
        }
    }

    pub(crate) fn draw_horde(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &CameraBinding,
        mesh: &CharacterMesh,
        draws: &CharacterDraws,
    ) {
        for index in 0..MAX_HORDE_POSE_BUCKETS {
            for (_, group) in draws.groups.iter().enumerate().filter(|(mask, _)| mask & 1 != 0) {
                self.draw_posed(pass, &camera.bind_group, mesh, index, group.horde[index].clone());
            }
        }
    }
}
