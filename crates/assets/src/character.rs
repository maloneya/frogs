//! Hierarchical, skinned character assets kept separate from baked static meshes.

use glam::{Mat3, Mat4, Vec2, Vec3};

use super::{
    BaseColorTexture, ImportError, MAX_GLB_BYTES, MAX_INDICES, MAX_VERTICES, exactly,
    import_material, node_transform,
};

/// Largest retained node hierarchy accepted for one character.
pub const MAX_CHARACTER_NODES: usize = 128;

/// Largest joint palette accepted by the first character renderer.
pub const MAX_JOINTS: usize = 64;

const _: () = assert!(MAX_CHARACTER_NODES <= u16::MAX as usize);
const _: () = assert!(MAX_JOINTS <= u16::MAX as usize);

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

/// One retained node in the character's asset-local hierarchy.
#[derive(Debug)]
pub struct CharacterNode {
    name: Box<str>,
    parent: Option<u16>,
    local_transform: Mat4,
}

impl CharacterNode {
    /// Unique authored node name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Parent node index, or `None` for the sole scene root.
    #[must_use]
    pub fn parent(&self) -> Option<usize> {
        self.parent.map(usize::from)
    }

    /// Authored bind-pose transform relative to the parent.
    #[must_use]
    pub fn local_transform(&self) -> Mat4 {
        self.local_transform
    }
}

/// One joint's link into the node hierarchy and its authored inverse bind.
#[derive(Debug)]
pub struct SkinJoint {
    node: u16,
    inverse_bind: Mat4,
}

impl SkinJoint {
    /// Node index whose transform drives this joint.
    #[must_use]
    pub fn node(&self) -> usize {
        usize::from(self.node)
    }

    /// Matrix taking mesh-local positions into this joint's bind space.
    #[must_use]
    pub fn inverse_bind(&self) -> Mat4 {
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
    bind_joint_matrices: Vec<JointMatrix>,
    mesh_node: u16,
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

    /// Complete retained node hierarchy in glTF node-index order.
    #[must_use]
    pub fn nodes(&self) -> &[CharacterNode] {
        &self.nodes
    }

    /// Joints in the same order vertex joint indices and GPU palettes use.
    #[must_use]
    pub fn joints(&self) -> &[SkinJoint] {
        &self.joints
    }

    /// Bind-pose transforms from mesh-local positions to asset-local positions.
    #[must_use]
    pub fn bind_joint_matrices(&self) -> &[JointMatrix] {
        &self.bind_joint_matrices
    }

    /// Node carrying the skinned mesh and skin reference.
    #[must_use]
    pub fn mesh_node(&self) -> usize {
        usize::from(self.mesh_node)
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

/// Imports the bounded one-mesh, one-skin character subset of binary glTF.
///
/// Unlike [`super::import_glb`], node transforms are retained. The importer
/// validates one rooted hierarchy, one named joint tree, inverse bind matrices,
/// and exactly four joint/weight lanes per vertex. Animation remains rejected.
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
            "extensions are not accepted in bind-pose characters",
        ));
    }
    exactly(
        document.scenes().count(),
        1,
        "exactly one scene is required",
    )?;
    exactly(document.meshes().count(), 1, "exactly one mesh is required")?;
    exactly(document.skins().count(), 1, "exactly one skin is required")?;
    exactly(
        document.buffers().count(),
        1,
        "exactly one embedded buffer is required",
    )?;
    exactly(
        document.accessors().count(),
        7,
        "exactly seven character accessors are required",
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
    if document.animations().next().is_some() || document.cameras().next().is_some() {
        return Err(ImportError::Unsupported(
            "cameras and animation are deferred",
        ));
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
        nodes[index] = Some(CharacterNode {
            name: name.into(),
            parent: parent.map(|value| value as u16),
            local_transform,
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
        bind_joint_matrices,
        mesh_node: mesh_node as u16,
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
    fn imports_blenders_named_joint_tree_and_bind_pose() {
        let character = import_character_glb(FIXTURE).unwrap();
        assert_eq!(
            (character.vertex_count(), character.index_count()),
            (144, 216)
        );
        assert_eq!(character.nodes().len(), 5);
        assert_eq!(character.joints().len(), 3);
        assert_eq!(
            character
                .joints()
                .iter()
                .map(|joint| character.nodes()[joint.node()].name())
                .collect::<Vec<_>>(),
            ["Root", "Spine", "Head"]
        );
        assert_eq!(
            character.nodes()[character.mesh_node()].name(),
            "BindPoseCharacter"
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
                .bind_joint_matrices()
                .iter()
                .all(|matrix| matrix.matrix().abs_diff_eq(Mat4::IDENTITY, 1.0e-5))
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
            mutate_json(FIXTURE, "\"joints\":[2,1,0]", "\"joints\":[3,1,0]"),
            mutate_json(
                FIXTURE,
                "\"mesh\":0,\"name\"",
                "\"mesh\":0,\"translation\":[1,0,0],\"name\"",
            ),
        ] {
            assert!(import_character_glb(&invalid).is_err());
        }
    }
}
