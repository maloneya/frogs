//! Hierarchical, skinned character assets kept separate from baked static meshes.

use arpg_core::LinearRgba;

use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
use std::sync::Arc;

use super::{
    BaseColorTexture, ImportError, MAX_GLB_BYTES, MAX_INDICES, MAX_VERTICES, exactly,
    import_material, import_image, node_transform,
};

/// Largest retained node hierarchy accepted for one character.
pub(crate) const MAX_CHARACTER_NODES: usize = 128;

/// Total drawable primitives in one character, including every mesh node.
pub const MAX_CHARACTER_PARTS: usize = 32;
/// Total material/image/texture catalog entries in one character.
pub const MAX_CHARACTER_MATERIALS: usize = 32;
/// Total decoded image bytes including all mip levels, counted once per image.
pub const MAX_CHARACTER_TEXTURE_BYTES: usize = 64 << 20;

/// Largest joint palette accepted by the first character renderer.
pub const MAX_JOINTS: usize = 64;

/// Largest number of transform channels accepted in one clip.
pub(crate) const MAX_ANIMATION_CHANNELS: usize = MAX_JOINTS * 3;

/// Largest named clip catalog admitted for one character.
pub(crate) const MAX_ANIMATION_CLIPS: usize = 16;

/// Largest key count accepted for one transform channel.
pub(crate) const MAX_KEYFRAMES_PER_CHANNEL: usize = 256;

/// Long clips are an authoring error for the initial action-sized path.
pub(crate) const MAX_CLIP_SECONDS: f32 = 60.0;

const _: () = assert!(MAX_CHARACTER_NODES <= u16::MAX as usize);
const _: () = assert!(MAX_JOINTS <= u16::MAX as usize);
const _: () = assert!(MAX_ANIMATION_CHANNELS <= u16::MAX as usize);
const _: () = assert!(MAX_ANIMATION_CLIPS <= u16::MAX as usize);

/// One skinned vertex in asset-local metres.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CharacterVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    joints: [u16; 4],
    weights: [f32; 4],
}

const _: () = assert!(size_of::<CharacterVertex>() == 56);

impl CharacterVertex {
    /// Mesh-local position before skinning.
    #[must_use]
    pub fn position(self) -> Vec3 {
        Vec3::from(self.position)
    }

    /// Mesh-local unit normal before skinning.
    #[must_use]
    pub fn normal(self) -> Vec3 {
        Vec3::from(self.normal)
    }

    /// Coordinates in glTF's upper-left-origin convention.
    #[must_use]
    pub fn uv(self) -> Vec2 {
        Vec2::from(self.uv)
    }

    /// Indices into this character's joint palette.
    #[must_use]
    pub fn joints(self) -> [u16; 4] {
        self.joints
    }

    /// Four finite, non-negative influences normalized at import.
    #[must_use]
    pub fn weights(self) -> [f32; 4] {
        self.weights
    }
}

#[derive(Clone, Copy, Debug)]
struct NodePose {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

/// One retained node in the character's asset-local hierarchy.
#[derive(Debug)]
struct CharacterNode {
    name: Box<str>,
    parent: Option<u16>,
    bind_pose: NodePose,
}

impl NodePose {
    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnimationInterpolation {
    Linear,
    Step,
}

#[derive(Debug)]
enum ChannelValues {
    Translations(Vec<Vec3>),
    Rotations(Vec<Quat>),
    Scales(Vec<Vec3>),
}

#[derive(Debug)]
struct AnimationChannel {
    node: u16,
    interpolation: AnimationInterpolation,
    times: Vec<f32>,
    values: ChannelValues,
}

impl AnimationChannel {
    fn node(&self) -> usize {
        usize::from(self.node)
    }
}

/// One named, bounded transform clip.
#[derive(Debug)]
pub struct AnimationClip {
    name: Box<str>,
    duration_seconds: f32,
    channels: Vec<AnimationChannel>,
}

/// Opaque index into one character asset's validated clip catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipId(u16);

/// Opaque index into one character asset's joint palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointId(u16);

impl AnimationClip {
    /// Unique authored clip name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Loop duration after subtracting the authored start time.
    #[must_use]
    pub fn duration_seconds(&self) -> f32 {
        self.duration_seconds
    }

    /// Number of transform channels in the clip.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Wraps continuous presentation time into this clip's loop interval.
    #[must_use]
    pub fn loop_time(&self, presentation_seconds: f64) -> f32 {
        if presentation_seconds.is_finite() {
            presentation_seconds.rem_euclid(f64::from(self.duration_seconds)) as f32
        } else {
            0.0
        }
    }
}

/// Reusable CPU scratch and GPU-ready matrices for one sampled character pose.
#[derive(Debug)]
pub struct CharacterPose {
    asset_id: CharacterAssetId,
    locals: Vec<NodePose>,
    globals: Vec<Mat4>,
    joints: Vec<JointMatrix>,
}

/// Validated per-node blend weights belonging to exactly one imported asset.
pub struct PoseMask {
    asset_id: CharacterAssetId,
    weights: Vec<f32>,
    parent_alignment: bool,
}

/// Identity of one imported character, retained by its poses and uploaded mesh.
///
/// Separate imports have separate identities, even for identical bytes. Keeping
/// the token alive prevents an unloaded asset's identity from being reused while
/// a pose or GPU mesh still refers to it. This is presentation-only state.
#[derive(Clone, Debug)]
pub struct CharacterAssetId(Arc<()>);

impl CharacterAssetId {
    /// Whether this pose was created by the same imported character.
    #[must_use]
    pub fn owns(&self, pose: &CharacterPose) -> bool {
        Arc::ptr_eq(&self.0, &pose.asset_id.0)
    }
}

impl CharacterPose {
    /// Joint palette consumed by the character renderer.
    #[must_use]
    pub fn joint_matrices(&self) -> &[JointMatrix] {
        &self.joints
    }
}

impl CharacterNode {
    fn name(&self) -> &str {
        &self.name
    }

    fn parent(&self) -> Option<usize> {
        self.parent.map(usize::from)
    }
}

/// One joint's link into the node hierarchy and its authored inverse bind.
#[derive(Debug)]
struct SkinJoint {
    node: u16,
    inverse_bind: Mat4,
}

impl SkinJoint {
    fn node(&self) -> usize {
        usize::from(self.node)
    }

    fn inverse_bind(&self) -> Mat4 {
        self.inverse_bind
    }
}

/// One column-major joint transform in the exact GPU uniform representation.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct JointMatrix {
    columns: [[f32; 4]; 4],
}

const _: () = assert!(size_of::<JointMatrix>() == 64);

impl JointMatrix {
    fn new(matrix: Mat4) -> Self {
        Self {
            columns: matrix.to_cols_array_2d(),
        }
    }

    /// Matrix represented by these GPU-ready columns.
    #[must_use]
    pub fn matrix(self) -> Mat4 {
        Mat4::from_cols_array_2d(&self.columns)
    }
}

/// One authored mesh primitive. Geometry shares buffers but each part keeps its
/// node name, primitive index and material; source geometry is not rebaked.
#[derive(Debug)]
pub struct CharacterPart {
    node_name: Box<str>,
    primitive_index: usize,
    vertices: std::ops::Range<usize>,
    indices: std::ops::Range<u32>,
    material: usize,
}

impl CharacterPart {
    /// Unique authored node name; multiple primitives can belong to that node.
    pub fn node_name(&self) -> &str { &self.node_name }
    /// Primitive index within the node's referenced mesh.
    pub fn primitive_index(&self) -> usize { self.primitive_index }
    /// Range in the owning character's vertex storage.
    pub fn vertices(&self) -> std::ops::Range<usize> { self.vertices.clone() }
    /// Range in the owning character's index storage. Indices address the shared
    /// vertex buffer directly, so GPU draws require no per-part base vertex.
    pub fn indices(&self) -> std::ops::Range<u32> { self.indices.clone() }
    /// Valid index into the owning character's material catalog.
    pub fn material_index(&self) -> usize { self.material }
}

/// One material referring to a decoded image shared by any number of parts.
#[derive(Debug)]
pub struct CharacterMaterial {
    base_color_factor: LinearRgba,
    image: usize,
}

impl CharacterMaterial {
    /// Linear colour multiplier applied after sampling the sRGB texture.
    pub fn base_color_factor(&self) -> LinearRgba { self.base_color_factor }
    /// Valid index into the owning character's image catalog.
    pub fn image_index(&self) -> usize { self.image }
}

/// One validated collection of mesh parts and materials sharing a hierarchy and skin.
#[derive(Debug)]
pub struct CharacterAsset {
    asset_id: CharacterAssetId,
    vertices: Vec<CharacterVertex>,
    indices: Vec<u32>,
    parts: Vec<CharacterPart>,
    materials: Vec<CharacterMaterial>,
    images: Vec<BaseColorTexture>,
    nodes: Vec<CharacterNode>,
    joints: Vec<SkinJoint>,
    hierarchy_order: Vec<u16>,
    clips: Vec<AnimationClip>,
    shadow: Option<CharacterShadow>,
}

/// Coarse caster validated to consume this character's existing joint palette.
#[derive(Debug)]
pub struct CharacterShadow {
    vertices: Vec<CharacterVertex>,
    indices: Vec<u32>,
}

impl CharacterShadow {
    /// Same vertex layout and joint order as the visible character.
    pub fn vertices(&self) -> &[CharacterVertex] { &self.vertices }
    /// Triangle indices into the caster geometry.
    pub fn indices(&self) -> &[u32] { &self.indices }
}

impl CharacterAsset {
    /// Optional depth-only geometry; never replaces visible parts or animation.
    pub fn shadow_mesh(&self) -> Option<&CharacterShadow> { self.shadow.as_ref() }

    /// Installs a smaller caster only after its complete rig matches this asset.
    /// Refusal preserves both the visible asset and any accepted caster.
    pub fn attach_shadow(&mut self, proxy: Self) -> Result<(), ImportError> {
        if proxy.indices.len() >= self.indices.len() {
            return Err(ImportError::Invalid("shadow mesh must have fewer triangles than the visible mesh"));
        }
        if proxy.joints.len() != self.joints.len() {
            return Err(ImportError::Invalid("shadow mesh must use the visible joint palette"));
        }
        let parent_joint = |asset: &Self, joint: &SkinJoint| {
            let mut parent = asset.nodes[joint.node()].parent;
            while let Some(node) = parent {
                if let Some(index) = asset.joints.iter().position(|joint| joint.node() == usize::from(node)) {
                    return Some(index);
                }
                parent = asset.nodes[usize::from(node)].parent;
            }
            None
        };
        let visible_bind = self.bind_pose();
        let proxy_bind = proxy.bind_pose();
        for (a, b) in self.joints.iter().zip(&proxy.joints) {
            if self.nodes[a.node()].name != proxy.nodes[b.node()].name
                || parent_joint(self, a) != parent_joint(&proxy, b)
                || !a.inverse_bind.abs_diff_eq(b.inverse_bind, 0.00001)
                || !visible_bind.globals[a.node()].abs_diff_eq(proxy_bind.globals[b.node()], 0.00001)
                || !self.nodes[a.node()].bind_pose.matrix().abs_diff_eq(proxy.nodes[b.node()].bind_pose.matrix(), 0.00001)
            {
                return Err(ImportError::Invalid("shadow mesh joint order, hierarchy and bind space must match"));
            }
        }
        self.shadow = Some(CharacterShadow { vertices: proxy.vertices, indices: proxy.indices });
        Ok(())
    }
    /// Retains this import's identity for resources derived from it.
    #[must_use]
    pub fn asset_id(&self) -> CharacterAssetId {
        self.asset_id.clone()
    }

    /// Skinned vertices in the renderer's fixed character layout.
    #[must_use]
    pub fn vertices(&self) -> &[CharacterVertex] {
        &self.vertices
    }

    /// Triangle indices normalized to `u32` at import.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Number reported without exposing storage ownership.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Number reported without exposing storage ownership.
    #[must_use]
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }

    /// Drawable parts in authored hierarchy and primitive order.
    pub fn parts(&self) -> &[CharacterPart] { &self.parts }
    /// Validated material catalog in glTF order.
    pub fn materials(&self) -> &[CharacterMaterial] { &self.materials }
    /// Decoded images in glTF order; shared images are stored once.
    pub fn images(&self) -> &[BaseColorTexture] { &self.images }
    /// Total retained decoded bytes, including mip levels.
    pub fn texture_bytes(&self) -> usize {
        self.images.iter().map(|image| image.rgba8_mip_chain().len()).sum()
    }

    /// Number of retained nodes in the character hierarchy.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of joints in the skin and sampled GPU palette.
    #[must_use]
    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// Number of named clips retained in authored order.
    #[must_use]
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Resolves an authored clip name once, before per-frame sampling.
    #[must_use]
    pub fn clip_named(&self, name: &str) -> Option<ClipId> {
        self.clips
            .iter()
            .position(|clip| clip.name.as_ref() == name)
            .map(|index| ClipId(index as u16))
    }

    /// Metadata for a clip handle minted by this asset.
    #[must_use]
    pub fn clip(&self, id: ClipId) -> Option<&AnimationClip> {
        self.clips.get(usize::from(id.0))
    }

    /// Resolves a skin joint by its retained node name.
    #[must_use]
    pub fn joint_named(&self, name: &str) -> Option<JointId> {
        self.joints
            .iter()
            .position(|joint| self.nodes[joint.node()].name() == name)
            .map(|index| JointId(index as u16))
    }

    /// Skin palette lane for an in-range joint handle.
    #[must_use]
    pub fn joint_index(&self, joint: JointId) -> Option<usize> {
        let index = usize::from(joint.0);
        (index < self.joints.len()).then_some(index)
    }

    /// Requires a direct joint attachment whose local transform is fixed in
    /// every clip. Fixed locals remain fixed through sampling and pose blends.
    pub fn validate_rigid_attachment(&self, parent: &str, child: &str, reference: ClipId) -> Result<(), &'static str> {
        let parent = self.joint_named(parent).ok_or("missing attachment parent")?;
        let child = self.joint_named(child).ok_or("missing attachment joint")?;
        let parent = self.joints[usize::from(parent.0)].node;
        let child = self.joints[usize::from(child.0)].node;
        let node = &self.nodes[usize::from(child)];
        if node.parent != Some(parent) { return Err("attachment is not a direct child of its owner"); }
        let clip = self.clip(reference).ok_or("missing attachment reference clip")?;
        let mut locals: Vec<_> = self.nodes.iter().map(|node| node.bind_pose).collect();
        Self::sample_locals(&self.nodes, clip, 0.0, &mut locals);
        let expected = locals[usize::from(child)];
        for clip in &self.clips {
            Self::sample_locals(&self.nodes, clip, 0.0, &mut locals);
            if !locals[usize::from(child)].matrix().abs_diff_eq(expected.matrix(), 1e-5) {
                return Err("attachment differs between clips");
            }
        }
        for channel in self.clips.iter().flat_map(|clip| &clip.channels).filter(|channel| channel.node == child) {
            let fixed = match &channel.values {
                ChannelValues::Translations(values) => values.iter().all(|value| value.abs_diff_eq(expected.translation, 1e-5)),
                ChannelValues::Scales(values) => values.iter().all(|value| value.abs_diff_eq(expected.scale, 1e-5)),
                ChannelValues::Rotations(values) => values.iter().all(|value| value.dot(expected.rotation).abs() > 1.0 - 1e-6),
            };
            if !fixed { return Err("attachment local transform is animated"); }
        }
        Ok(())
    }

    /// Sampled asset-local transform of one joint.
    #[must_use]
    pub fn joint_transform(&self, pose: &CharacterPose, joint: JointId) -> Option<Mat4> {
        if !self.asset_id.owns(pose) {
            return None;
        }
        let joint = self.joints.get(usize::from(joint.0))?;
        pose.globals.get(joint.node()).copied()
    }

    /// Builds reusable sampling storage initialized to the authored bind pose.
    #[must_use]
    pub fn bind_pose(&self) -> CharacterPose {
        let mut pose = CharacterPose {
            asset_id: self.asset_id(),
            locals: self.nodes.iter().map(|node| node.bind_pose).collect(),
            globals: vec![Mat4::IDENTITY; self.nodes.len()],
            joints: vec![JointMatrix::new(Mat4::IDENTITY); self.joints.len()],
        };
        self.rebuild_pose(&mut pose);
        pose
    }

    /// Samples one clip at a bounded time and rebuilds `pose`.
    ///
    /// Reuses the pose's allocations. Returns false without changing the pose
    /// for an invalid clip or a pose belonging to another import, even if its
    /// skeleton has the same dimensions. Non-finite time selects the clip start.
    pub fn sample(&self, clip: ClipId, seconds: f32, pose: &mut CharacterPose) -> bool {
        if !self.asset_id.owns(pose) {
            return false;
        }
        let Some(clip) = self.clips.get(usize::from(clip.0)) else {
            return false;
        };
        Self::sample_locals(&self.nodes, clip, seconds, &mut pose.locals);
        self.rebuild_pose(pose);
        true
    }

    /// A mask blending every node, for locomotion transitions.
    pub fn full_mask(&self) -> PoseMask {
        PoseMask { asset_id: self.asset_id.clone(), weights: vec![1.0; self.nodes.len()], parent_alignment: true }
    }

    /// Builds a sparse mask from unique joint names and finite weights in [0, 1].
    /// Unlisted joints keep the base pose. Names are resolved here so a joint
    /// index from a different skeleton cannot silently select another body part.
    pub fn pose_mask(&self, joints: &[(&str, f32)]) -> Result<PoseMask, &'static str> {
        let mut weights = vec![0.0; self.nodes.len()];
        let mut seen = vec![false; self.nodes.len()];
        for &(name, weight) in joints {
            if !weight.is_finite() || !(0.0..=1.0).contains(&weight) { return Err("invalid mask weight"); }
            let joint = self.joint_named(name).ok_or("missing mask joint")?;
            let node = self.joints[usize::from(joint.0)].node();
            if seen[node] { return Err("duplicate mask joint"); }
            seen[node] = true;
            weights[node] = weight;
        }
        // Alignment reads unmasked parents from the base pose. Reject a layer
        // that re-enters below an unmasked gap in another selected subtree,
        // since that parent's final rotation would differ from the base.
        let parent_alignment = self.nodes.iter().enumerate().all(|(index, node)| {
            if weights[index] == 0.0 || node.parent.is_some_and(|p| weights[usize::from(p)] > 0.0) { return true; }
            let mut parent = node.parent;
            while let Some(index) = parent.map(usize::from) {
                if weights[index] > 0.0 { return false; }
                parent = self.nodes[index].parent;
            }
            true
        });
        Ok(PoseMask { asset_id: self.asset_id.clone(), weights, parent_alignment })
    }

    /// Builds a layer mask whose selected subtrees have no unmasked gaps.
    /// Validate this when admitting assets that require parent-aligned layers.
    pub fn pose_mask_parent_aligned(&self, joints: &[(&str, f32)]) -> Result<PoseMask, &'static str> {
        let mask = self.pose_mask(joints)?;
        if !mask.parent_alignment { return Err("mask re-enters below an unmasked parent"); }
        Ok(mask)
    }

    /// Blends local transforms, then rebuilds the hierarchy and skin matrices.
    /// Rejects mismatched assets and invalid weights before changing the target.
    /// No allocation occurs during blending.
    pub fn blend(&self, target: &mut CharacterPose, source: &CharacterPose, mask: &PoseMask, amount: f32) -> bool {
        self.blend_locals(target, source, mask, amount, false)
    }

    /// Blends a layer while preserving its orientation above unmasked parents.
    /// A turning pelvis can retain locomotion while the chest/arms keep the
    /// overlay's asset-space aim. Translations remain relative to the base body.
    /// Rejects masks re-entering a selected subtree below an unmasked gap.
    /// Refusal preserves the complete target, as with [`Self::blend`].
    pub fn blend_parent_aligned(&self, target: &mut CharacterPose, source: &CharacterPose, mask: &PoseMask, amount: f32) -> bool {
        self.blend_locals(target, source, mask, amount, true)
    }

    fn blend_locals(&self, target: &mut CharacterPose, source: &CharacterPose, mask: &PoseMask, amount: f32, align_parents: bool) -> bool {
        if !self.asset_id.owns(target) || !self.asset_id.owns(source)
            || !Arc::ptr_eq(&self.asset_id.0, &mask.asset_id.0)
            || (align_parents && !mask.parent_alignment)
            || !amount.is_finite() || !(0.0..=1.0).contains(&amount) { return false; }
        for (index, ((base, overlay), weight)) in target.locals.iter_mut().zip(&source.locals).zip(&mask.weights).enumerate() {
            let mut overlay = *overlay;
            if align_parents && *weight > 0.0
                && let Some(parent) = self.nodes[index].parent.map(usize::from)
                && mask.weights[parent] == 0.0
            {
                let base_parent = target.globals[parent].to_scale_rotation_translation().1;
                let source_parent = source.globals[parent].to_scale_rotation_translation().1;
                overlay.rotation = (base_parent.inverse() * source_parent * overlay.rotation).normalize();
            }
            let weight = weight * amount;
            if weight == 0.0 { continue; }
            if weight == 1.0 { *base = overlay; continue; }
            base.translation = base.translation.lerp(overlay.translation, weight);
            base.rotation = base.rotation.slerp(overlay.rotation, weight).normalize();
            base.scale = base.scale.lerp(overlay.scale, weight);
        }
        self.rebuild_pose(target);
        true
    }

    fn sample_locals(
        nodes: &[CharacterNode],
        clip: &AnimationClip,
        seconds: f32,
        locals: &mut [NodePose],
    ) {
        for (local, node) in locals.iter_mut().zip(nodes) {
            *local = node.bind_pose;
        }
        let time = if seconds.is_finite() {
            seconds.clamp(0.0, clip.duration_seconds)
        } else {
            0.0
        };
        for channel in &clip.channels {
            channel.sample(time, &mut locals[channel.node()]);
        }
    }

    fn rebuild_pose(&self, pose: &mut CharacterPose) {
        for &node in &self.hierarchy_order {
            let node = usize::from(node);
            let local = pose.locals[node].matrix();
            pose.globals[node] = self.nodes[node]
                .parent()
                .map_or(local, |parent| pose.globals[parent] * local);
        }
        for (out, joint) in pose.joints.iter_mut().zip(&self.joints) {
            *out = JointMatrix::new(pose.globals[joint.node()] * joint.inverse_bind());
        }
    }
}

impl AnimationChannel {
    fn sample(&self, time: f32, pose: &mut NodePose) {
        let upper = self.times.partition_point(|&key| key <= time);
        let left = upper.saturating_sub(1).min(self.times.len() - 1);
        let right = upper.min(self.times.len() - 1);
        let amount = if self.interpolation == AnimationInterpolation::Step || left == right {
            0.0
        } else {
            (time - self.times[left]) / (self.times[right] - self.times[left])
        };
        match &self.values {
            ChannelValues::Translations(values) => {
                pose.translation = values[left].lerp(values[right], amount);
            }
            ChannelValues::Rotations(values) => {
                pose.rotation = values[left].slerp(values[right], amount).normalize();
            }
            ChannelValues::Scales(values) => {
                pose.scale = values[left].lerp(values[right], amount);
            }
        }
    }
}

fn is_affine(matrix: Mat4) -> bool {
    matrix.x_axis.w.abs() <= f32::EPSILON
        && matrix.y_axis.w.abs() <= f32::EPSILON
        && matrix.z_axis.w.abs() <= f32::EPSILON
        && (matrix.w_axis.w - 1.0).abs() <= f32::EPSILON
}

fn is_ancestor(ancestor: usize, mut node: usize, parents: &[Option<usize>]) -> bool {
    loop {
        if node == ancestor {
            return true;
        }
        let Some(parent) = parents[node] else {
            return false;
        };
        node = parent;
    }
}

fn import_animation<'a>(
    animation: gltf::Animation<'a>,
    blob: &[u8],
    joint_membership: &[bool],
) -> Result<(AnimationClip, Vec<gltf::Accessor<'a>>), ImportError> {
    let name = animation
        .name()
        .filter(|name| !name.is_empty())
        .ok_or(ImportError::Invalid("the animation must be named"))?;
    let channel_count = animation.channels().count();
    if channel_count == 0 || channel_count > MAX_ANIMATION_CHANNELS {
        return Err(ImportError::Capacity("animation channel count"));
    }
    exactly(
        animation.samplers().count(),
        channel_count,
        "each animation channel must own one sampler",
    )?;

    let mut sampler_used = vec![false; channel_count];
    let mut targets = vec![[false; 3]; joint_membership.len()];
    let mut accessors = Vec::with_capacity(channel_count * 2);
    let mut channels = Vec::with_capacity(channel_count);
    let mut clip_range: Option<(f32, f32)> = None;

    for channel in animation.channels() {
        let target = channel.target();
        let node = target.node().index();
        if !joint_membership[node] {
            return Err(ImportError::Unsupported(
                "animation channels may target only skin joints",
            ));
        }
        let property = target.property();
        let property_index = match property {
            gltf::animation::Property::Translation => 0,
            gltf::animation::Property::Rotation => 1,
            gltf::animation::Property::Scale => 2,
            gltf::animation::Property::MorphTargetWeights => {
                return Err(ImportError::Unsupported(
                    "morph target animation is deferred",
                ));
            }
        };
        if std::mem::replace(&mut targets[node][property_index], true) {
            return Err(ImportError::Invalid(
                "a clip cannot animate one node property twice",
            ));
        }

        let sampler = channel.sampler();
        if std::mem::replace(&mut sampler_used[sampler.index()], true) {
            return Err(ImportError::Unsupported(
                "animation samplers cannot be shared between channels",
            ));
        }
        let interpolation = match sampler.interpolation() {
            gltf::animation::Interpolation::Linear => AnimationInterpolation::Linear,
            gltf::animation::Interpolation::Step => AnimationInterpolation::Step,
            gltf::animation::Interpolation::CubicSpline => {
                return Err(ImportError::Unsupported(
                    "cubic-spline animation is deferred",
                ));
            }
        };
        let input = sampler.input();
        let output = sampler.output();
        if input.data_type() != gltf::accessor::DataType::F32
            || input.dimensions() != gltf::accessor::Dimensions::Scalar
            || input.normalized()
        {
            return Err(ImportError::Unsupported(
                "animation times must be unnormalized FLOAT scalars",
            ));
        }
        if input.count() < 2 || input.count() > MAX_KEYFRAMES_PER_CHANNEL {
            return Err(ImportError::Capacity("animation keyframe count"));
        }
        let expected_dimensions = match property {
            gltf::animation::Property::Translation | gltf::animation::Property::Scale => {
                gltf::accessor::Dimensions::Vec3
            }
            gltf::animation::Property::Rotation => gltf::accessor::Dimensions::Vec4,
            gltf::animation::Property::MorphTargetWeights => unreachable!("rejected above"),
        };
        if output.data_type() != gltf::accessor::DataType::F32
            || output.dimensions() != expected_dimensions
            || output.normalized()
            || output.count() != input.count()
        {
            return Err(ImportError::Unsupported(
                "animation outputs must be matching unnormalized FLOAT vectors",
            ));
        }

        let reader = channel.reader(|_| Some(blob));
        let mut times: Vec<f32> = reader
            .read_inputs()
            .ok_or(ImportError::Invalid("read animation times"))?
            .collect();
        if times.len() != input.count()
            || times.iter().any(|time| !time.is_finite() || *time < 0.0)
            || times.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ImportError::Invalid(
                "animation times must be finite, non-negative and strictly increasing",
            ));
        }
        let start = times[0];
        let end = *times.last().expect("at least two keys checked");
        let duration = end - start;
        if !duration.is_finite() || duration <= 0.0 || duration > MAX_CLIP_SECONDS {
            return Err(ImportError::Capacity("animation duration"));
        }
        if let Some((clip_start, clip_end)) = clip_range {
            if (start - clip_start).abs() > 1.0e-5 || (end - clip_end).abs() > 1.0e-5 {
                return Err(ImportError::Unsupported(
                    "all channels must span the same clip range",
                ));
            }
        } else {
            clip_range = Some((start, end));
        }
        for time in &mut times {
            *time -= start;
        }

        let outputs = reader
            .read_outputs()
            .ok_or(ImportError::Invalid("read animation values"))?;
        let values = match outputs {
            gltf::animation::util::ReadOutputs::Translations(values) => {
                let values: Vec<Vec3> = values.map(Vec3::from).collect();
                if values.len() != times.len() || values.iter().any(|value| !value.is_finite()) {
                    return Err(ImportError::Invalid(
                        "animation translations must be finite",
                    ));
                }
                ChannelValues::Translations(values)
            }
            gltf::animation::util::ReadOutputs::Rotations(values) => {
                let values: Vec<Quat> = values.into_f32().map(Quat::from_array).collect();
                if values.len() != times.len()
                    || values.iter().any(|value| {
                        !value.is_finite() || (value.length_squared() - 1.0).abs() > 1.0e-3
                    })
                {
                    return Err(ImportError::Invalid(
                        "animation rotations must be finite unit quaternions",
                    ));
                }
                ChannelValues::Rotations(values.into_iter().map(Quat::normalize).collect())
            }
            gltf::animation::util::ReadOutputs::Scales(values) => {
                let values: Vec<Vec3> = values.map(Vec3::from).collect();
                if values.len() != times.len()
                    || values.iter().any(|value| {
                        !value.is_finite()
                            || value.min_element() <= 1.0e-8
                            || value.max_element() - value.min_element() > 1.0e-5
                    })
                {
                    return Err(ImportError::Invalid(
                        "animation scales must be finite, positive and uniform",
                    ));
                }
                ChannelValues::Scales(values)
            }
            gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                unreachable!("morph target channels rejected above")
            }
        };
        accessors.extend([input, output]);
        channels.push(AnimationChannel {
            node: node as u16,
            interpolation,
            times,
            values,
        });
    }
    debug_assert!(sampler_used.into_iter().all(|used| used));
    let (start, end) = clip_range.expect("non-empty channels checked");
    Ok((
        AnimationClip {
            name: name.into(),
            duration_seconds: end - start,
            channels,
        },
        accessors,
    ))
}

/// Imports bounded mesh parts and materials sharing one skin in binary glTF.
///
/// Unlike [`super::import_glb`], node transforms are retained. The importer
/// validates one rooted hierarchy, one named joint tree, inverse bind matrices,
/// exactly four joint/weight lanes per vertex and a bounded named clip catalog.
pub fn import_character_glb(bytes: &[u8]) -> Result<CharacterAsset, ImportError> {
    if bytes.len() > MAX_GLB_BYTES {
        return Err(ImportError::Capacity("file bytes"));
    }
    if !bytes.starts_with(b"glTF") {
        return Err(ImportError::Unsupported("binary .glb input is required"));
    }

    let gltf = gltf::Gltf::from_slice(bytes).map_err(ImportError::Parse)?;
    let document = &gltf.document;
    if document.extensions_used().next().is_some()
        || document.extensions_required().next().is_some()
    {
        return Err(ImportError::Unsupported(
            "extensions are not accepted in characters",
        ));
    }
    exactly(
        document.scenes().count(),
        1,
        "exactly one scene is required",
    )?;
    if document.meshes().count() == 0 || document.meshes().count() > MAX_CHARACTER_PARTS {
        return Err(ImportError::Capacity("character mesh count"));
    }
    exactly(document.skins().count(), 1, "exactly one skin is required")?;
    let animation_count = document.animations().count();
    if animation_count == 0 || animation_count > MAX_ANIMATION_CLIPS {
        return Err(ImportError::Capacity("animation clip count"));
    }
    exactly(
        document.buffers().count(),
        1,
        "exactly one embedded buffer is required",
    )?;
    for count in [document.images().count(), document.materials().count(),
        document.samplers().count(), document.textures().count()] {
        if count == 0 || count > MAX_CHARACTER_MATERIALS {
            return Err(ImportError::Capacity("character material catalog"));
        }
    }
    if document.cameras().next().is_some() {
        return Err(ImportError::Unsupported("cameras are deferred"));
    }
    if document
        .accessors()
        .any(|accessor| accessor.sparse().is_some())
    {
        return Err(ImportError::Unsupported("sparse accessors are deferred"));
    }

    let buffer = document.buffers().next().expect("count checked");
    if !matches!(buffer.source(), gltf::buffer::Source::Bin) {
        return Err(ImportError::Unsupported(
            "external and data URI buffers are rejected",
        ));
    }
    let blob = gltf
        .blob
        .as_deref()
        .ok_or(ImportError::Invalid("embedded BIN chunk is missing"))?;

    let node_count = document.nodes().count();
    if node_count == 0 || node_count > MAX_CHARACTER_NODES {
        return Err(ImportError::Capacity("character node count"));
    }
    let scene = document.default_scene().ok_or(ImportError::Unsupported(
        "the sole scene must be selected as the default",
    ))?;
    exactly(
        scene.nodes().count(),
        1,
        "the scene must have one root node",
    )?;

    let mut nodes: Vec<Option<CharacterNode>> =
        std::iter::repeat_with(|| None).take(node_count).collect();
    let mut parents = vec![None; node_count];
    let mut globals = vec![Mat4::IDENTITY; node_count];
    let mut hierarchy_order = Vec::with_capacity(node_count);
    let mut stack = vec![(
        scene.nodes().next().expect("count checked"),
        None,
        Mat4::IDENTITY,
    )];
    let mut mesh_nodes = Vec::new();
    let mut used_meshes = vec![false; document.meshes().count()];
    while let Some((node, parent, parent_global)) = stack.pop() {
        let index = node.index();
        if nodes[index].is_some() {
            return Err(ImportError::Invalid(
                "character nodes must form one tree without shared children",
            ));
        }
        let local_transform = node_transform(node.clone())?;
        let (scale, rotation, translation) = local_transform.to_scale_rotation_translation();
        if !scale.is_finite()
            || scale.min_element() <= 1.0e-8
            || scale.max_element() - scale.min_element() > 1.0e-5
            || !rotation.is_finite()
            || (rotation.length_squared() - 1.0).abs() > 1.0e-4
            || !translation.is_finite()
            || !Mat4::from_scale_rotation_translation(scale, rotation, translation)
                .abs_diff_eq(local_transform, 1.0e-5)
        {
            return Err(ImportError::Unsupported(
                "character node transforms must be finite positive uniform TRS without shear",
            ));
        }
        let global = parent_global * local_transform;
        let determinant = Mat3::from_mat4(global).determinant();
        if !determinant.is_finite() || determinant <= 1.0e-8 {
            return Err(ImportError::Invalid(
                "character hierarchy transforms must preserve orientation",
            ));
        }
        let name = node
            .name()
            .filter(|name| !name.is_empty())
            .ok_or(ImportError::Invalid("every character node must be named"))?;
        if nodes
            .iter()
            .flatten()
            .any(|existing| existing.name() == name)
        {
            return Err(ImportError::Invalid("character node names must be unique"));
        }
        if node.camera().is_some() {
            return Err(ImportError::Unsupported(
                "character nodes cannot contain cameras",
            ));
        }
        if let Some(mesh) = node.mesh() {
            mesh_nodes.push((index, mesh.clone()));
            used_meshes[mesh.index()] = true;
            if node.skin().map(|skin| skin.index()) != Some(0) {
                return Err(ImportError::Invalid(
                    "the mesh node must reference the sole skin",
                ));
            }
        } else if node.skin().is_some() {
            return Err(ImportError::Invalid(
                "only the mesh node may reference a skin",
            ));
        }
        parents[index] = parent;
        globals[index] = global;
        hierarchy_order.push(index as u16);
        nodes[index] = Some(CharacterNode {
            name: name.into(),
            parent: parent.map(|value| value as u16),
            bind_pose: NodePose {
                translation,
                rotation,
                scale,
            },
        });
        let mut children: Vec<_> = node.children().collect();
        children.reverse();
        for child in children {
            stack.push((child, Some(index), global));
        }
    }
    if nodes.iter().any(Option::is_none) {
        return Err(ImportError::Unsupported(
            "unused character nodes are rejected",
        ));
    }
    let nodes: Vec<CharacterNode> = nodes.into_iter().map(Option::unwrap).collect();
    if mesh_nodes.is_empty() || used_meshes.contains(&false) {
        return Err(ImportError::Invalid("every character mesh must be referenced"));
    }
    // All parts share one bind space and joint palette. Other mesh bind spaces
    // and additional skins are outside this character contract.
    if mesh_nodes.iter().any(|(node, _)| !globals[*node].abs_diff_eq(Mat4::IDENTITY, 1.0e-6)) {
        return Err(ImportError::Unsupported("skinned mesh nodes must have identity global transforms"));
    }
    let part_count: usize = mesh_nodes.iter().map(|(_, mesh)| mesh.primitives().count()).sum();
    if part_count == 0 || part_count > MAX_CHARACTER_PARTS {
        return Err(ImportError::Capacity("character part count"));
    }

    let skin = document.skins().next().expect("count checked");
    let joint_nodes: Vec<usize> = skin.joints().map(|node| node.index()).collect();
    if joint_nodes.is_empty() || joint_nodes.len() > MAX_JOINTS {
        return Err(ImportError::Capacity("joint count"));
    }
    let mut joint_membership = vec![false; node_count];
    for &node in &joint_nodes {
        if std::mem::replace(&mut joint_membership[node], true) {
            return Err(ImportError::Invalid("skin joints must be unique"));
        }
    }
    let joint_roots = joint_nodes
        .iter()
        .filter(|&&node| {
            parents[node]
                .is_none_or(|parent| !is_ancestor_of_any_joint(parent, &joint_membership, &parents))
        })
        .count();
    if joint_roots != 1 {
        return Err(ImportError::Unsupported(
            "skin joints must form one rooted tree",
        ));
    }
    if let Some(skeleton) = skin.skeleton()
        && joint_nodes
            .iter()
            .any(|&joint| !is_ancestor(skeleton.index(), joint, &parents))
    {
        return Err(ImportError::Invalid(
            "the declared skeleton root must contain every joint",
        ));
    }
    let mut clips = Vec::with_capacity(animation_count);
    let mut animation_accessors = Vec::new();
    for animation in document.animations() {
        let (clip, accessors) = import_animation(animation, blob, &joint_membership)?;
        if clips
            .iter()
            .any(|existing: &AnimationClip| existing.name == clip.name)
        {
            return Err(ImportError::Invalid("animation clip names must be unique"));
        }
        clips.push(clip);
        animation_accessors.extend(accessors);
    }
    let inverse_accessor = skin
        .inverse_bind_matrices()
        .ok_or(ImportError::Invalid("inverseBindMatrices are required"))?;
    if inverse_accessor.count() != joint_nodes.len()
        || inverse_accessor.data_type() != gltf::accessor::DataType::F32
        || inverse_accessor.dimensions() != gltf::accessor::Dimensions::Mat4
        || inverse_accessor.normalized()
    {
        return Err(ImportError::Invalid(
            "inverseBindMatrices must be one FLOAT MAT4 per joint",
        ));
    }

    let inverse_binds: Vec<Mat4> = skin
        .reader(|_| Some(blob))
        .read_inverse_bind_matrices()
        .ok_or(ImportError::Invalid("read inverseBindMatrices"))?
        .map(|columns| Mat4::from_cols_array_2d(&columns))
        .collect();
    if inverse_binds.len() != joint_nodes.len()
        || inverse_binds.iter().any(|matrix| {
            !matrix.is_finite()
                || !is_affine(*matrix)
                || Mat3::from_mat4(*matrix).determinant().abs() <= 1.0e-8
        })
    {
        return Err(ImportError::Invalid(
            "inverse bind matrices must be finite, affine and invertible",
        ));
    }
    let bind_joint_matrices: Vec<JointMatrix> = joint_nodes
        .iter()
        .zip(&inverse_binds)
        .map(|(&node, &inverse_bind)| JointMatrix::new(globals[node] * inverse_bind))
        .collect();
    let joints: Vec<SkinJoint> = joint_nodes
        .iter()
        .zip(inverse_binds)
        .map(|(&node, inverse_bind)| SkinJoint {
            node: node as u16,
            inverse_bind,
        })
        .collect();


    let mut used_accessors = vec![inverse_accessor];
    used_accessors.extend(animation_accessors);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut parts = Vec::with_capacity(part_count);
    let mut used_materials = vec![false; document.materials().count()];
    for (node, mesh) in mesh_nodes {
        for primitive in mesh.primitives() {
            let material = primitive.material().index().ok_or(ImportError::Invalid("an explicit material is required"))?;
            used_materials[material] = true;
            let vertex_start = vertices.len();
            let index_start = indices.len();
            let primitive_index = primitive.index();
            let imported = import_part(
                primitive, blob, &bind_joint_matrices, vertex_start, index_start,
            )?;
            vertices.extend(imported.vertices);
            indices.extend(imported.indices.into_iter().map(|index| index + vertex_start as u32));
            parts.push(CharacterPart {
                node_name: nodes[node].name.clone(), primitive_index,
                vertices: vertex_start..vertices.len(), indices: index_start as u32..indices.len() as u32, material,
            });
            used_accessors.extend(imported.accessors);
        }
    }
    if used_materials.contains(&false) {
        return Err(ImportError::Unsupported("unused character materials are rejected"));
    }
    let mut materials = Vec::with_capacity(used_materials.len());
    let mut used_images = vec![false; document.images().count()];
    let mut used_textures = vec![false; document.textures().count()];
    let mut used_samplers = vec![false; document.samplers().count()];
    for material in document.materials() {
        let source = import_material(material)?;
        let image = source.texture.source().index();
        used_images[image] = true;
        used_textures[source.texture.index()] = true;
        used_samplers[source.texture.sampler().index().expect("validated sampler")] = true;
        materials.push(CharacterMaterial { base_color_factor: source.base_color_factor, image });
    }
    if used_images.contains(&false) || used_textures.contains(&false) || used_samplers.contains(&false) {
        return Err(ImportError::Unsupported("unused character texture resources are rejected"));
    }
    let mut images = Vec::with_capacity(used_images.len());
    let mut remaining_bytes = MAX_CHARACTER_TEXTURE_BYTES;
    let mut used_views = Vec::new();
    for image in document.images() {
        let (image, view) = import_image(image, blob, remaining_bytes)?;
        remaining_bytes -= image.rgba8_mip_chain().len();
        images.push(image);
        used_views.push(view);
    }
    used_accessors.sort_by_key(gltf::Accessor::index);
    used_accessors.dedup_by_key(|accessor| accessor.index());
    exactly(document.accessors().count(), used_accessors.len(), "unused character accessors are rejected")?;
    for accessor in used_accessors {
        used_views.push(accessor.view().ok_or(ImportError::Unsupported("sparse accessors are deferred"))?);
    }
    used_views.sort_by_key(gltf::buffer::View::index);
    used_views.dedup_by_key(|view| view.index());
    exactly(document.views().count(), used_views.len(), "unused buffer views are rejected")?;
    if used_views.iter().any(|view| view.buffer().index() != 0) {
        return Err(ImportError::Invalid("character data must use the embedded buffer"));
    }
    Ok(CharacterAsset {
        asset_id: CharacterAssetId(Arc::new(())), vertices, indices, parts, materials, images,
        nodes, joints, hierarchy_order, clips, shadow: None,
    })
}

struct ImportedPart<'a> {
    vertices: Vec<CharacterVertex>,
    indices: Vec<u32>,
    accessors: Vec<gltf::Accessor<'a>>,
}

fn import_part<'a>(
    primitive: gltf::Primitive<'a>, blob: &[u8], bind_joint_matrices: &[JointMatrix],
    vertex_start: usize, index_start: usize,
) -> Result<ImportedPart<'a>, ImportError> {
    if primitive.mode() != gltf::mesh::Mode::Triangles {
        return Err(ImportError::Unsupported(
            "only TRIANGLES primitives are accepted",
        ));
    }
    if primitive.morph_targets().next().is_some() {
        return Err(ImportError::Unsupported("morph targets are deferred"));
    }
    exactly(
        primitive.attributes().count(),
        5,
        "only POSITION, NORMAL, TEXCOORD_0, JOINTS_0 and WEIGHTS_0 are accepted",
    )?;

    let positions = primitive
        .get(&gltf::Semantic::Positions)
        .ok_or(ImportError::Invalid("POSITION is required"))?;
    let normals = primitive
        .get(&gltf::Semantic::Normals)
        .ok_or(ImportError::Invalid("NORMAL is required"))?;
    let uvs = primitive
        .get(&gltf::Semantic::TexCoords(0))
        .ok_or(ImportError::Invalid("TEXCOORD_0 is required"))?;
    let joint_indices = primitive
        .get(&gltf::Semantic::Joints(0))
        .ok_or(ImportError::Invalid("JOINTS_0 is required"))?;
    let weights = primitive
        .get(&gltf::Semantic::Weights(0))
        .ok_or(ImportError::Invalid("WEIGHTS_0 is required"))?;
    let indices = primitive
        .indices()
        .ok_or(ImportError::Unsupported("indices are required"))?;
    let attribute_count = positions.count();
    if attribute_count == 0
        || [
            normals.count(),
            uvs.count(),
            joint_indices.count(),
            weights.count(),
        ]
        .into_iter()
        .any(|count| count != attribute_count)
    {
        return Err(ImportError::Invalid(
            "all character vertex attribute counts must be equal and nonzero",
        ));
    }
    if attribute_count > MAX_VERTICES - vertex_start {
        return Err(ImportError::Capacity("vertex count"));
    }
    if uvs.data_type() != gltf::accessor::DataType::F32
        || uvs.dimensions() != gltf::accessor::Dimensions::Vec2
        || uvs.normalized()
    {
        return Err(ImportError::Unsupported(
            "TEXCOORD_0 must be unnormalized FLOAT VEC2",
        ));
    }
    if !matches!(
        joint_indices.data_type(),
        gltf::accessor::DataType::U8 | gltf::accessor::DataType::U16
    ) || joint_indices.dimensions() != gltf::accessor::Dimensions::Vec4
        || joint_indices.normalized()
    {
        return Err(ImportError::Unsupported(
            "JOINTS_0 must be unnormalized unsigned-byte or unsigned-short VEC4",
        ));
    }
    if weights.data_type() != gltf::accessor::DataType::F32
        || weights.dimensions() != gltf::accessor::Dimensions::Vec4
        || weights.normalized()
    {
        return Err(ImportError::Unsupported(
            "WEIGHTS_0 must be unnormalized FLOAT VEC4",
        ));
    }
    if indices.count() == 0 || !indices.count().is_multiple_of(3) {
        return Err(ImportError::Invalid(
            "index count must describe complete triangles",
        ));
    }
    if indices.count() > MAX_INDICES - index_start {
        return Err(ImportError::Capacity("index count"));
    }
    if !matches!(
        indices.data_type(),
        gltf::accessor::DataType::U16 | gltf::accessor::DataType::U32
    ) {
        return Err(ImportError::Unsupported(
            "indices must be unsigned 16- or 32-bit",
        ));
    }

    let reader = primitive.reader(|_| Some(blob));
    let source_positions = reader
        .read_positions()
        .ok_or(ImportError::Invalid("read POSITION"))?;
    let source_normals = reader
        .read_normals()
        .ok_or(ImportError::Invalid("read NORMAL"))?;
    let source_uvs = reader
        .read_tex_coords(0)
        .ok_or(ImportError::Invalid("read TEXCOORD_0"))?
        .into_f32();
    let source_joints = reader
        .read_joints(0)
        .ok_or(ImportError::Invalid("read JOINTS_0"))?
        .into_u16();
    let source_weights = reader
        .read_weights(0)
        .ok_or(ImportError::Invalid("read WEIGHTS_0"))?
        .into_f32();
    let mut vertices = Vec::with_capacity(attribute_count);
    for ((((position, normal), uv), vertex_joints), vertex_weights) in source_positions
        .zip(source_normals)
        .zip(source_uvs)
        .zip(source_joints)
        .zip(source_weights)
    {
        let position = Vec3::from(position);
        let normal = Vec3::from(normal);
        let uv = Vec2::from(uv);
        let mut vertex_weights = vertex_weights;
        let weight_sum: f32 = vertex_weights.iter().sum();
        if !position.is_finite()
            || !normal.is_finite()
            || !uv.is_finite()
            || (normal.length_squared() - 1.0).abs() > 1.0e-3
            || vertex_weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight < 0.0)
            || !weight_sum.is_finite()
            || weight_sum <= 1.0e-8
            || vertex_joints
                .iter()
                .any(|&joint| usize::from(joint) >= bind_joint_matrices.len())
        {
            return Err(ImportError::Invalid(
                "character vertices, normals, UVs, joints and weights must be finite and valid",
            ));
        }
        for weight in &mut vertex_weights {
            *weight /= weight_sum;
        }
        vertices.push(CharacterVertex {
            position: position.into(),
            normal: normal.into(),
            uv: uv.into(),
            joints: vertex_joints,
            weights: vertex_weights,
        });
    }
    if vertices.len() != attribute_count {
        return Err(ImportError::Invalid(
            "character vertex accessor ended early",
        ));
    }

    let imported_indices: Vec<u32> = reader
        .read_indices()
        .ok_or(ImportError::Invalid("read indices"))?
        .into_u32()
        .collect();
    if imported_indices.len() != indices.count()
        || imported_indices
            .iter()
            .any(|&index| index as usize >= vertices.len())
    {
        return Err(ImportError::Invalid(
            "indices must stay within the vertex range",
        ));
    }

    for vertex in &vertices {
        let skinned = vertex.joints().into_iter().zip(vertex.weights()).fold(
            Vec3::ZERO,
            |sum, (joint, weight)| {
                sum + bind_joint_matrices[usize::from(joint)]
                    .matrix()
                    .transform_point3(vertex.position())
                    * weight
            },
        );
        let expected = vertex.position();
        if !skinned.is_finite() || skinned.distance(expected) > 1.0e-4 {
            return Err(ImportError::Invalid(
                "skin does not reproduce the authored bind pose",
            ));
        }
    }

    Ok(ImportedPart { vertices, indices: imported_indices, accessors: vec![positions, normals, uvs, joint_indices, weights, indices] })
}

fn is_ancestor_of_any_joint(mut node: usize, joints: &[bool], parents: &[Option<usize>]) -> bool {
    loop {
        if joints[node] {
            return true;
        }
        let Some(parent) = parents[node] else {
            return false;
        };
        node = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::mutate_json;

    const FIXTURE: &[u8] = include_bytes!("../../../assets/fixtures/blender-bind-pose.glb");
    const STATIC_FIXTURE: &[u8] = include_bytes!("../../../assets/fixtures/static-preview.glb");

    #[test]
    fn refused_shadow_preserves_the_accepted_caster_and_visible_asset() {
        const ENEMY: &[u8] = include_bytes!("../../../assets/characters/basic-enemy/basic-enemy.glb");
        const SHADOW: &[u8] = include_bytes!("../../../assets/characters/basic-enemy/basic-enemy.shadow.glb");
        let mut owner = import_character_glb(ENEMY).unwrap();
        owner.attach_shadow(import_character_glb(SHADOW).unwrap()).unwrap();
        let before = bytemuck::cast_slice::<_, u8>(owner.shadow_mesh().unwrap().vertices()).to_vec();
        let visible = bytemuck::cast_slice::<_, u8>(owner.vertices()).to_vec();
        for fault in 0..4 {
            let mut proxy = import_character_glb(SHADOW).unwrap();
            match fault {
                0 => proxy.joints.swap(0, 1),
                1 => proxy.joints[0].inverse_bind = Mat4::from_translation(Vec3::X),
                2 => { let node = proxy.joints[1].node(); proxy.nodes[node].name = "OtherJoint".into(); }
                3 => proxy.indices.resize(owner.index_count(), 0),
                _ => unreachable!(),
            }
            assert!(owner.attach_shadow(proxy).is_err());
            assert_eq!(before, bytemuck::cast_slice::<_, u8>(owner.shadow_mesh().unwrap().vertices()));
            assert_eq!(visible, bytemuck::cast_slice::<_, u8>(owner.vertices()));
        }
        assert!(owner.attach_shadow(import_character_glb(FIXTURE).unwrap()).is_err());
    }

    #[test]
    fn foreign_pose_is_rejected_even_with_identical_skeleton_dimensions() {
        let owner = import_character_glb(FIXTURE).unwrap();
        let other = import_character_glb(FIXTURE).unwrap();
        let mut pose = owner.bind_pose();
        let idle = owner.clip_named("Idle").unwrap();
        assert!(owner.sample(idle, 0.5, &mut pose));
        let before = bytemuck::cast_slice::<_, u8>(pose.joint_matrices()).to_vec();
        assert!(!other.sample(other.clip_named("Idle").unwrap(), 0.0, &mut pose));
        assert_eq!(bytemuck::cast_slice::<_, u8>(pose.joint_matrices()), before);
        assert!(owner.asset_id().owns(&pose));
        assert!(!other.asset_id().owns(&pose));
        assert!(other.joint_transform(&pose, other.joint_named("Weapon").unwrap()).is_none());
        assert!(owner.sample(idle, 0.0, &mut pose));

        let identity = owner.asset_id();
        drop(owner);
        assert!(identity.owns(&pose), "identity survives the source asset");
    }

    #[test]
    fn rigid_attachment_checks_hierarchy_all_keys_and_missing_clip_channels() {
        let source = include_bytes!("../../../assets/characters/basic-player/basic-player.glb");
        for fault in 0..3 {
            let mut asset = import_character_glb(source).unwrap();
            let idle = asset.clip_named("Idle").unwrap();
            let joint = asset.joint_named("RightIndex1").unwrap();
            let node = asset.joints[usize::from(joint.0)].node;
            assert!(asset.validate_rigid_attachment("RightHand", "RightIndex1", idle).is_ok());
            match fault {
                0 => asset.nodes[usize::from(node)].parent = None,
                1 => {
                    let clip = asset.clip_named("Run").unwrap();
                    let channel = asset.clips[usize::from(clip.0)].channels.iter_mut()
                        .find(|channel| channel.node == node && matches!(channel.values, ChannelValues::Rotations(_))).unwrap();
                    if let ChannelValues::Rotations(values) = &mut channel.values {
                        let index = values.len() / 2;
                        values[index] = (Quat::from_rotation_x(0.2) * values[index]).normalize();
                    }
                }
                2 => {
                    let clip = asset.clip_named("Run").unwrap();
                    asset.clips[usize::from(clip.0)].channels.retain(|channel|
                        channel.node != node || !matches!(channel.values, ChannelValues::Rotations(_)));
                }
                _ => unreachable!(),
            }
            assert!(asset.validate_rigid_attachment("RightHand", "RightIndex1", idle).is_err(), "fault {fault}");
        }
    }

    #[test]
    fn swordsman_blade_and_two_hand_handle_share_one_rigid_attachment() {
        let character = import_character_glb(include_bytes!(
            "../../../assets/characters/basic-player/basic-player.glb"
        ))
        .unwrap();
        let weapon = character.joint_named("Weapon").unwrap();
        let bind = character.bind_pose();
        let to_joint = character.joint_transform(&bind, weapon).unwrap().inverse();
        let mut count = 0;
        let mut near = f32::INFINITY;
        let mut far = f32::NEG_INFINITY;
        for vertex in character.vertices() {
            let weight: f32 = vertex
                .joints()
                .into_iter()
                .zip(vertex.weights())
                .filter_map(|(joint, weight)| (joint == weapon.0).then_some(weight))
                .sum();
            if weight == 0.0 {
                continue;
            }
            assert_eq!(weight, 1.0, "the sword must not deform between joints");
            let local = to_joint.transform_point3(vertex.position());
            near = near.min(local.y);
            far = far.max(local.y);
            count += 1;
        }
        assert!(count >= 24, "Weapon must carry geometry, not just a named bone");
        assert!((-0.31..=-0.20).contains(&near), "the pommel must extend behind both grips: {near}");
        assert!((1.20..1.25).contains(&far), "the longsword tip must extend along local +Y: {far}");
        let socket = character.joint_named("BeamOrigin").unwrap();
        let tip = to_joint.transform_point3(character.joint_transform(&bind, socket).unwrap().transform_point3(Vec3::ZERO));
        assert!((tip.y - far).abs() < 1e-5, "the attachment must meet the actual blade tip");
    }

    #[test]
    fn imports_blenders_named_joint_tree_bind_pose_and_clip_catalog() {
        let character = import_character_glb(FIXTURE).unwrap();
        assert_eq!(
            (character.vertex_count(), character.index_count()),
            (144, 216)
        );
        assert_eq!(character.node_count(), 6);
        assert_eq!(character.joint_count(), 4);
        assert_eq!(
            character
                .joints
                .iter()
                .map(|joint| character.nodes[joint.node()].name())
                .collect::<Vec<_>>(),
            ["Root", "Spine", "Head", "Weapon"]
        );
        assert_eq!(
            (
                character.images()[0].width(),
                character.images()[0].height(),
                character.images()[0].mip_level_count(),
            ),
            (16, 16, 5)
        );
        assert!(
            character
                .vertices()
                .iter()
                .all(|vertex| { (vertex.weights().into_iter().sum::<f32>() - 1.0).abs() < 1.0e-6 })
        );
        assert!(
            character
                .bind_pose()
                .joint_matrices()
                .iter()
                .all(|matrix| { matrix.matrix().abs_diff_eq(Mat4::IDENTITY, 1.0e-5) })
        );
        assert_eq!(character.clip_count(), 6);
        let idle = character.clip_named("Idle").unwrap();
        let clip = character.clip(idle).unwrap();
        assert_eq!(clip.name(), "Idle");
        assert!((clip.duration_seconds() - 1.0).abs() < 1.0e-5);
        assert_eq!(clip.channel_count(), 12);
        assert_eq!(
            clip.channels
                .iter()
                .filter(|channel| channel.interpolation == AnimationInterpolation::Linear)
                .count(),
            1
        );
        assert_eq!(
            clip.channels
                .iter()
                .filter(|channel| channel.interpolation == AnimationInterpolation::Step)
                .count(),
            11
        );
        assert!(character.joint_named("Weapon").is_some());
        for name in [
            "Run",
            "AttackCleave",
            "AttackSlam",
        ] {
            assert!(character.clip_named(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn linear_and_step_channels_obey_their_two_edges() {
        let mut pose = NodePose {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        };
        let mut channel = AnimationChannel {
            node: 0,
            interpolation: AnimationInterpolation::Linear,
            times: vec![0.0, 1.0],
            values: ChannelValues::Translations(vec![Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0)]),
        };
        channel.sample(0.25, &mut pose);
        assert_eq!(pose.translation, Vec3::new(0.5, 0.0, 0.0));

        channel.interpolation = AnimationInterpolation::Step;
        channel.sample(0.25, &mut pose);
        assert_eq!(pose.translation, Vec3::ZERO);
        channel.sample(1.0, &mut pose);
        assert_eq!(pose.translation, Vec3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn clip_sampling_wraps_continuously_and_rebuilds_the_joint_palette() {
        let character = import_character_glb(FIXTURE).unwrap();
        let mut pose = character.bind_pose();
        let idle = character.clip_named("Idle").unwrap();
        assert!(character.sample(idle, 0.5, &mut pose));
        assert!(
            pose.joint_matrices()
                .iter()
                .any(|matrix| !matrix.matrix().abs_diff_eq(Mat4::IDENTITY, 1.0e-3))
        );
        let halfway: Vec<Mat4> = pose
            .joint_matrices()
            .iter()
            .map(|matrix| matrix.matrix())
            .collect();

        let sampled = character.clip(idle).unwrap().loop_time(1.5);
        assert!((sampled - 0.5).abs() < 1.0e-5);
        assert!(character.sample(idle, sampled, &mut pose));
        assert!(
            pose.joint_matrices()
                .iter()
                .zip(halfway)
                .all(|(actual, expected)| actual.matrix().abs_diff_eq(expected, 1.0e-5))
        );

        assert!(character.sample(idle, f32::NAN, &mut pose));
        assert!(
            pose.joint_matrices()
                .iter()
                .all(|matrix| matrix.matrix().abs_diff_eq(Mat4::IDENTITY, 1.0e-5))
        );

        let weapon = character.joint_named("Weapon").unwrap();
        assert!(
            character
                .joint_transform(&pose, weapon)
                .unwrap()
                .is_finite()
        );
    }

    #[test]
    fn static_and_character_imports_remain_distinct_boundaries() {
        assert!(import_character_glb(STATIC_FIXTURE).is_err());
        assert!(crate::import_glb(FIXTURE).is_err());
    }

    #[test]
    fn malformed_hierarchies_and_skins_are_rejected() {
        for invalid in [
            mutate_json(FIXTURE, "\"name\":\"Head\"", "\"name\":\"Spine\""),
            mutate_json(
                FIXTURE,
                "\"inverseBindMatrices\":6",
                "\"inverseBindMatrixes\":6",
            ),
            mutate_json(FIXTURE, "\"joints\":[3,2,0,1]", "\"joints\":[3,2,0,0]"),
            mutate_json(
                FIXTURE,
                "\"mesh\":0,\"name\"",
                "\"mesh\":0,\"translation\":[1,0,0],\"name\"",
            ),
            mutate_json(
                FIXTURE,
                "\"name\":\"AttackCleave\"",
                "\"name\":\"AttackSlam\"",
            ),
            mutate_json(
                FIXTURE,
                "\"name\":\"Head\",\"translation\"",
                "\"name\":\"Head\",\"scale\":[1,2,1],\"translation\"",
            ),
            mutate_json(
                FIXTURE,
                "\"target\":{\"node\":2,\"path\":\"translation\"}",
                "\"target\":{\"node\":3,\"path\":\"translation\"}",
            ),
            mutate_json(
                FIXTURE,
                "\"interpolation\":\"LINEAR\"",
                "\"interpolation\":\"CUBICSPLINE\"",
            ),
        ] {
            assert!(import_character_glb(&invalid).is_err());
        }
    }
    const MULTIPART: &[u8] = include_bytes!("../../../assets/fixtures/blender-multipart.glb");
    const MULTIPART_REFERENCE: &[u8] = include_bytes!("../../../assets/fixtures/blender-multipart-reference.glb");

    fn edit_glb(bytes: &[u8], edit: impl FnOnce(&mut gltf::json::Value, &mut Vec<u8>)) -> Vec<u8> {
        let mut glb = gltf::binary::Glb::from_slice(bytes).unwrap();
        let mut root: gltf::json::Value = gltf::json::deserialize::from_slice(&glb.json).unwrap();
        let mut bin = glb.bin.take().unwrap().into_owned();
        edit(&mut root, &mut bin);
        root["buffers"][0]["byteLength"] = bin.len().into();
        glb.json = gltf::json::serialize::to_vec(&root).unwrap().into();
        glb.bin = Some(bin.into());
        glb.to_vec().unwrap()
    }

    #[test]
    fn multipart_ranges_keep_each_index_inside_its_part_and_share_one_pose() {
        let asset = import_character_glb(MULTIPART).unwrap();
        assert_eq!(asset.parts().len(), 10);
        assert_eq!(asset.materials().len(), 3);
        assert_eq!(asset.images().len(), 3);
        assert_eq!(asset.joint_count(), 4);
        let mut next_vertex = 0;
        let mut next_index = 0;
        for part in asset.parts() {
            assert_eq!(part.vertices().start, next_vertex);
            assert_eq!(part.indices().start, next_index);
            assert!(part.material_index() < asset.materials().len());
            for index in &asset.indices()[part.indices().start as usize..part.indices().end as usize] {
                assert!(part.vertices().contains(&(*index as usize)));
            }
            next_vertex = part.vertices().end;
            next_index = part.indices().end;
        }
        assert_eq!(next_vertex, asset.vertex_count());
        assert_eq!(next_index as usize, asset.index_count());
        // Source nodes/primitives may be in different orders. Compare the actual
        // animated triangle positions, not joint indices or storage order.
        let reference = import_character_glb(MULTIPART_REFERENCE).unwrap();
        for clip in ["Idle", "Run", "AttackCleave", "AttackSlam"] {
            for seconds in [0.0, 0.2, 0.5, 0.8] {
                let triangles = |asset: &CharacterAsset| {
                    let mut pose = asset.bind_pose();
                    assert!(asset.sample(asset.clip_named(clip).unwrap(), seconds, &mut pose));
                    let points: Vec<_> = asset.vertices().iter().map(|vertex| {
                        vertex.joints().into_iter().zip(vertex.weights()).fold(Vec3::ZERO, |sum, (joint, weight)| {
                            sum + pose.joint_matrices()[usize::from(joint)].matrix().transform_point3(vertex.position()) * weight
                        }).to_array().map(|value| (value * 100_000.0).round() as i32)
                    }).collect();
                    let mut triangles: Vec<_> = asset.indices().as_chunks::<3>().0.iter().map(|indices| {
                        let mut corners = [points[indices[0] as usize], points[indices[1] as usize], points[indices[2] as usize]];
                        corners.sort_unstable();
                        corners
                    }).collect();
                    triangles.sort_unstable();
                    triangles
                };
                assert_eq!(triangles(&asset), triangles(&reference), "{clip} at {seconds}");
            }
        }
    }

    #[test]
    fn malformed_later_parts_and_materials_refuse_the_whole_asset() {
        let invalid = edit_glb(MULTIPART, |root, bin| {
            let meshes = root["meshes"].as_array().unwrap();
            let accessor = meshes.last().unwrap()["primitives"][0]["indices"].as_u64().unwrap() as usize;
            let a = &root["accessors"][accessor];
            let view = &root["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
            let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
            bin[offset..offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        });
        assert!(matches!(import_character_glb(&invalid), Err(ImportError::Invalid("indices must stay within the vertex range"))));
        let invalid = edit_glb(MULTIPART, |root, _| {
            root["materials"].as_array_mut().unwrap().last_mut().unwrap()["doubleSided"] = true.into();
        });
        assert!(matches!(import_character_glb(&invalid), Err(ImportError::Unsupported(_))));
        let invalid = edit_glb(MULTIPART, |root, _| {
            let second_skin = root["skins"][0].clone();
            root["skins"].as_array_mut().unwrap().push(second_skin);
        });
        assert!(matches!(import_character_glb(&invalid), Err(ImportError::Unsupported(_))));
    }

    #[test]
    fn character_budgets_cover_all_parts_and_materials() {
        let too_many_parts = edit_glb(MULTIPART_REFERENCE, |root, _| {
            let primitive = root["meshes"][0]["primitives"][0].clone();
            root["meshes"][0]["primitives"] = vec![primitive; MAX_CHARACTER_PARTS + 1].into();
        });
        assert!(matches!(import_character_glb(&too_many_parts), Err(ImportError::Capacity("character part count"))));
        let too_many_materials = edit_glb(MULTIPART_REFERENCE, |root, _| {
            let material = root["materials"][0].clone();
            root["materials"] = vec![material; MAX_CHARACTER_MATERIALS + 1].into();
        });
        assert!(matches!(import_character_glb(&too_many_materials), Err(ImportError::Capacity("character material catalog"))));
        // Reuse a small buffer in many primitives. Each primitive is legal on
        // its own, but their combined decoded geometry must exceed the budget.
        for vertex_budget in [true, false] {
            let oversized = edit_glb(MULTIPART_REFERENCE, |root, bin| {
                let primitive = root["meshes"][0]["primitives"][0].clone();
                let accessors: Vec<usize> = if vertex_budget {
                    primitive["attributes"].as_object().unwrap().values().map(|v| v.as_u64().unwrap() as usize).collect()
                } else { vec![primitive["indices"].as_u64().unwrap() as usize] };
                for index in accessors {
                    let a = root["accessors"][index].clone();
                    let view = root["bufferViews"][a["bufferView"].as_u64().unwrap() as usize].clone();
                    let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
                    let components = match a["type"].as_str().unwrap() { "SCALAR" => 1, "VEC2" => 2, "VEC3" => 3, "VEC4" => 4, _ => unreachable!() };
                    let width = match a["componentType"].as_u64().unwrap() { 5121 => 1, 5123 => 2, 5125 | 5126 => 4, _ => unreachable!() };
                    let element = bin[offset..offset + width * components].to_vec();
                    let count = if vertex_budget { MAX_VERTICES / MAX_CHARACTER_PARTS + 1 } else { (MAX_INDICES / MAX_CHARACTER_PARTS / 3 + 1) * 3 };
                    while !bin.len().is_multiple_of(4) { bin.push(0); }
                    let mut new_view = view;
                    new_view["byteOffset"] = bin.len().into();
                    new_view["byteLength"] = (count * element.len()).into();
                    new_view.as_object_mut().unwrap().remove("byteStride");
                    for _ in 0..count { bin.extend_from_slice(&element); }
                    root["accessors"][index]["bufferView"] = root["bufferViews"].as_array().unwrap().len().into();
                    root["accessors"][index]["byteOffset"] = 0.into();
                    root["accessors"][index]["count"] = count.into();
                    root["bufferViews"].as_array_mut().unwrap().push(new_view);
                }
                root["meshes"][0]["primitives"] = vec![primitive; MAX_CHARACTER_PARTS].into();
            });
            let expected = if vertex_budget { "vertex count" } else { "index count" };
            assert!(matches!(import_character_glb(&oversized), Err(ImportError::Capacity(reason)) if reason == expected), "{expected}");
        }
    }

    #[test]
    fn texture_catalog_budget_counts_images_once_and_refuses_before_decode() {
        let asset = import_character_glb(MULTIPART).unwrap();
        assert_eq!(asset.texture_bytes(), asset.images().iter().map(|i| i.rgba8_mip_chain().len()).sum::<usize>());
        assert!(asset.parts().len() > asset.images().len());
        let shared = edit_glb(MULTIPART, |root, _| {
            let mut material = root["materials"][0].clone();
            material["pbrMetallicRoughness"]["baseColorFactor"] = vec![0.5, 0.25, 0.75, 1.0].into();
            root["materials"].as_array_mut().unwrap().push(material);
            root["meshes"][0]["primitives"][0]["material"] = 3.into();
        });
        let shared = import_character_glb(&shared).unwrap();
        assert_eq!(shared.materials().len(), 4);
        assert_eq!(shared.images().len(), 3);
        assert_eq!(shared.materials()[0].image_index(), shared.materials()[3].image_index());
        assert_eq!(shared.materials()[3].base_color_factor().to_array(), [0.5, 0.25, 0.75, 1.0]);
        assert_eq!(shared.texture_bytes(), asset.texture_bytes());
        let gltf = gltf::Gltf::from_slice(MULTIPART).unwrap();
        let blob = gltf.blob.as_ref().unwrap();
        let image = gltf.document.images().next().unwrap();
        let bytes = asset.images()[0].rgba8_mip_chain().len();
        assert!(matches!(import_image(image.clone(), blob, bytes - 1), Err(ImportError::Capacity("total character texture bytes"))));
        assert!(import_image(image, blob, bytes).is_ok());
    }

}
