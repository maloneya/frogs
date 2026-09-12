//! Hierarchical, skinned character assets kept separate from baked static meshes.

use glam::{Mat3, Mat4, Quat, Vec2, Vec3};

use super::{
    BaseColorTexture, ImportError, MAX_GLB_BYTES, MAX_INDICES, MAX_VERTICES, exactly,
    import_material, node_transform,
};

/// Largest retained node hierarchy accepted for one character.
pub(crate) const MAX_CHARACTER_NODES: usize = 128;

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
    locals: Vec<NodePose>,
    globals: Vec<Mat4>,
    joints: Vec<JointMatrix>,
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

/// One validated character mesh, material, node hierarchy and skin.
#[derive(Debug)]
pub struct CharacterAsset {
    vertices: Vec<CharacterVertex>,
    indices: Vec<u32>,
    base_color_factor: [f32; 4],
    base_color_texture: BaseColorTexture,
    nodes: Vec<CharacterNode>,
    joints: Vec<SkinJoint>,
    hierarchy_order: Vec<u16>,
    clips: Vec<AnimationClip>,
}

impl CharacterAsset {
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

    /// Linear multiplier authored on the glTF base-colour material.
    #[must_use]
    pub fn base_color_factor(&self) -> [f32; 4] {
        self.base_color_factor
    }

    /// Decoded base-colour image and generated mip chain.
    #[must_use]
    pub fn base_color_texture(&self) -> &BaseColorTexture {
        &self.base_color_texture
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

    /// Sampled asset-local transform of one joint.
    #[must_use]
    pub fn joint_transform(&self, pose: &CharacterPose, joint: JointId) -> Option<Mat4> {
        let joint = self.joints.get(usize::from(joint.0))?;
        pose.globals.get(joint.node()).copied()
    }

    /// Builds reusable sampling storage initialized to the authored bind pose.
    #[must_use]
    pub fn bind_pose(&self) -> CharacterPose {
        let mut pose = CharacterPose {
            locals: self.nodes.iter().map(|node| node.bind_pose).collect(),
            globals: vec![Mat4::IDENTITY; self.nodes.len()],
            joints: vec![JointMatrix::new(Mat4::IDENTITY); self.joints.len()],
        };
        self.rebuild_pose(&mut pose);
        pose
    }

    /// Samples one clip at a bounded time and rebuilds `pose`.
    ///
    /// A matching-size pose reuses its allocations; other shapes are replaced.
    /// Non-finite time selects the clip start.
    pub fn sample(&self, clip: ClipId, seconds: f32, pose: &mut CharacterPose) -> bool {
        let Some(clip) = self.clips.get(usize::from(clip.0)) else {
            return false;
        };
        self.ensure_pose_shape(pose);
        Self::sample_locals(&self.nodes, clip, seconds, &mut pose.locals);
        self.rebuild_pose(pose);
        true
    }

    fn ensure_pose_shape(&self, pose: &mut CharacterPose) {
        if pose.locals.len() != self.nodes.len()
            || pose.globals.len() != self.nodes.len()
            || pose.joints.len() != self.joints.len()
        {
            *pose = self.bind_pose();
        }
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

/// Imports the bounded one-mesh, one-skin character subset of binary glTF.
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
    exactly(document.meshes().count(), 1, "exactly one mesh is required")?;
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
    exactly(
        document.images().count(),
        1,
        "exactly one image is required",
    )?;
    exactly(
        document.materials().count(),
        1,
        "exactly one material is required",
    )?;
    exactly(
        document.samplers().count(),
        1,
        "exactly one sampler is required",
    )?;
    exactly(
        document.textures().count(),
        1,
        "exactly one texture is required",
    )?;
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
    let mut mesh_node = None;
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
            if mesh.index() != 0 || mesh_node.replace(index).is_some() {
                return Err(ImportError::Invalid(
                    "exactly one node must reference the sole mesh",
                ));
            }
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
    let mesh_node = mesh_node.ok_or(ImportError::Invalid(
        "one node must reference the character mesh",
    ))?;
    if !globals[mesh_node].abs_diff_eq(Mat4::IDENTITY, 1.0e-6) {
        return Err(ImportError::Unsupported(
            "the skinned mesh node must have an identity global transform",
        ));
    }

    let mesh = document.meshes().next().expect("count checked");
    exactly(
        mesh.primitives().count(),
        1,
        "exactly one mesh primitive is required",
    )?;
    let primitive = mesh.primitives().next().expect("count checked");
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
    let material = import_material(&primitive, blob)?;

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
    if attribute_count > MAX_VERTICES {
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
    if indices.count() > MAX_INDICES {
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

    let mut used_views = [
        positions.view(),
        normals.view(),
        uvs.view(),
        joint_indices.view(),
        weights.view(),
        indices.view(),
        inverse_accessor.view(),
    ]
    .into_iter()
    .map(|view| view.ok_or(ImportError::Unsupported("sparse accessors are deferred")))
    .collect::<Result<Vec<_>, _>>()?;
    for accessor in &animation_accessors {
        used_views.push(
            accessor
                .view()
                .ok_or(ImportError::Unsupported("sparse accessors are deferred"))?,
        );
    }
    used_views.push(material.image_view.clone());
    used_views.sort_by_key(gltf::buffer::View::index);
    used_views.dedup_by_key(|view| view.index());
    exactly(
        document.views().count(),
        used_views.len(),
        "unused buffer views are rejected",
    )?;
    if used_views.iter().any(|view| view.buffer().index() != 0) {
        return Err(ImportError::Invalid(
            "character data must use the embedded buffer",
        ));
    }
    let mut used_accessors = vec![
        positions.clone(),
        normals.clone(),
        uvs.clone(),
        joint_indices.clone(),
        weights.clone(),
        indices.clone(),
        inverse_accessor.clone(),
    ];
    used_accessors.extend(animation_accessors);
    used_accessors.sort_by_key(gltf::Accessor::index);
    used_accessors.dedup_by_key(|accessor| accessor.index());
    exactly(
        document.accessors().count(),
        used_accessors.len(),
        "unused character accessors are rejected",
    )?;

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
                .any(|&joint| usize::from(joint) >= joints.len())
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

    Ok(CharacterAsset {
        vertices,
        indices: imported_indices,
        base_color_factor: material.base_color_factor,
        base_color_texture: material.base_color_texture,
        nodes,
        joints,
        hierarchy_order,
        clips,
    })
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

    const FIXTURE: &[u8] = include_bytes!("../../../assets/fixtures/blender-bind-pose.glb");
    const STATIC_FIXTURE: &[u8] = include_bytes!("../../../assets/fixtures/static-preview.glb");

    #[test]
    fn demo_sword_is_rigid_geometry_extending_from_its_grip() {
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
        assert!(near.abs() < 0.1, "the sword grip must meet its joint origin");
        assert!(far - near > 0.8, "the authored sword must extend along local +Y");
    }

    fn mutate_json(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&bytes[20..20 + json_len]).unwrap();
        assert!(json.contains(from), "fixture no longer contains {from:?}");
        let mut json = json.replace(from, to).into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let old_bin = 20 + json_len;
        let bin_len = u32::from_le_bytes(bytes[old_bin..old_bin + 4].try_into().unwrap()) as usize;
        let bin = &bytes[old_bin + 8..old_bin + 8 + bin_len];
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2_u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4e4f_534a_u32.to_le_bytes());
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004e_4942_u32.to_le_bytes());
        out.extend_from_slice(bin);
        out
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
                character.base_color_texture().width(),
                character.base_color_texture().height(),
                character.base_color_texture().mip_level_count(),
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
        assert_eq!(character.clip_count(), 8);
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
            "AttackBasic",
            "AttackThrust",
            "AttackSweep",
            "AttackHeavySweep",
            "AttackCleave",
            "AttackCrowdBreaker",
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
                "\"name\":\"AttackBasic\"",
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
}
