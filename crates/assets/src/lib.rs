//! Validated CPU-side assets imported from interchange formats.
//!
//! This crate is the one-way door from glTF into the engine. Format-specific
//! handles stay private; callers receive finite, bounded static geometry or a
//! retained character hierarchy, skin and clip. It owns neither file selection
//! nor GPU state.

use glam::{Mat3, Mat4, Vec2, Vec3};

mod character;

pub use character::import_character_glb;
pub use character::{
    AnimationClip, CharacterAsset, CharacterPose, CharacterVertex, JointMatrix, MAX_JOINTS,
};

/// Largest binary glTF accepted by the first asset path: eight MiB.
///
/// Public so the app can reject an oversized file from metadata before reading
/// it. [`import_glb`] checks again because byte callers cannot be trusted to
/// have come through that adapter.
pub const MAX_GLB_BYTES: usize = 8 << 20;

/// A static preview cannot allocate more than this many vertices.
pub const MAX_VERTICES: usize = 262_144;

/// Three indices per vertex leaves room for ordinary split-normal meshes while
/// bounding both CPU and GPU allocation.
pub const MAX_INDICES: usize = MAX_VERTICES * 3;

/// Largest accepted dimension of the single base-colour texture.
pub const MAX_TEXTURE_DIMENSION: u32 = 2_048;

/// Decoded RGBA8 storage is bounded independently of compressed file size.
pub const MAX_TEXTURE_PIXELS: usize =
    MAX_TEXTURE_DIMENSION as usize * MAX_TEXTURE_DIMENSION as usize;

/// Maximum decoded allocation for tightly packed RGBA8 texels.
pub const MAX_TEXTURE_RGBA_BYTES: usize = MAX_TEXTURE_PIXELS * 4;

/// Maximum allocation for the base image and every generated mip level.
pub const MAX_TEXTURE_MIP_RGBA_BYTES: usize = (MAX_TEXTURE_RGBA_BYTES * 4).div_ceil(3);

const _: () = assert!(MAX_VERTICES <= u32::MAX as usize);
const _: () = assert!(MAX_INDICES <= u32::MAX as usize);

/// One validated vertex in metres, with a unit-length world-space normal.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

const _: () = assert!(size_of::<Vertex>() == 8 * size_of::<f32>());

impl Vertex {
    /// Position after the selected scene node's transforms have been applied.
    #[must_use]
    pub fn position(self) -> Vec3 {
        Vec3::from(self.position)
    }

    /// Unit-length normal after inverse-transpose transformation.
    #[must_use]
    pub fn normal(self) -> Vec3 {
        Vec3::from(self.normal)
    }

    /// Coordinates in glTF's upper-left-origin convention.
    #[must_use]
    pub fn uv(self) -> Vec2 {
        Vec2::from(self.uv)
    }
}

/// One decoded opaque base-colour image.
#[derive(Debug)]
pub struct BaseColorTexture {
    width: u32,
    height: u32,
    rgba8_mip_chain: Vec<u8>,
}

impl BaseColorTexture {
    /// Pixel width after decoding and capacity validation.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Pixel height after decoding and capacity validation.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Tightly packed rows from top to bottom, four bytes per texel.
    #[must_use]
    pub fn rgba8(&self) -> &[u8] {
        let base_len = self.width as usize * self.height as usize * 4;
        &self.rgba8_mip_chain[..base_len]
    }

    /// Number of levels in the complete chain, including the base image.
    #[must_use]
    pub fn mip_level_count(&self) -> u32 {
        u32::BITS - self.width.max(self.height).leading_zeros()
    }

    /// Consecutive tightly packed RGBA8 levels, largest to smallest.
    #[must_use]
    pub fn rgba8_mip_chain(&self) -> &[u8] {
        &self.rgba8_mip_chain
    }
}

/// The complete CPU representation accepted by the Stage 2 renderer.
#[derive(Debug)]
pub struct StaticMesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    base_color_factor: [f32; 4],
    base_color_texture: BaseColorTexture,
}

impl StaticMesh {
    /// Transformed vertices in the renderer's fixed vertex layout.
    #[must_use]
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// Triangle indices, normalized to `u32` at import.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Number reported through the harness without exposing storage ownership.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Number reported through the harness without exposing storage ownership.
    #[must_use]
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }

    /// Linear multiplier authored on the glTF base-colour material.
    #[must_use]
    pub fn base_color_factor(&self) -> [f32; 4] {
        self.base_color_factor
    }

    /// The sole base-colour image, still encoded as RGBA8 sRGB texels.
    #[must_use]
    pub fn base_color_texture(&self) -> &BaseColorTexture {
        &self.base_color_texture
    }
}

/// Why a `.glb` did not cross the supported-subset boundary.
#[derive(Debug)]
pub enum ImportError {
    /// The bytes are not structurally valid glTF 2.0.
    Parse(gltf::Error),
    /// The embedded PNG is malformed or exceeds its decoder budget.
    Image(png::DecodingError),
    /// Valid glTF that asks for a feature outside the current contract.
    Unsupported(&'static str),
    /// A supported field contains values unsafe or meaningless to render.
    Invalid(&'static str),
    /// A declared or actual allocation exceeds the fixed preview budget.
    Capacity(&'static str),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(out, "parse GLB: {error}"),
            Self::Image(error) => write!(out, "decode base-colour PNG: {error}"),
            Self::Unsupported(reason) => write!(out, "unsupported GLB: {reason}"),
            Self::Invalid(reason) => write!(out, "invalid GLB: {reason}"),
            Self::Capacity(reason) => write!(out, "GLB exceeds preview capacity: {reason}"),
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Image(error) => Some(error),
            Self::Unsupported(_) | Self::Invalid(_) | Self::Capacity(_) => None,
        }
    }
}

pub(crate) fn exactly(
    count: usize,
    expected: usize,
    reason: &'static str,
) -> Result<(), ImportError> {
    if count == expected {
        Ok(())
    } else {
        Err(ImportError::Unsupported(reason))
    }
}

pub(crate) fn node_transform(node: gltf::Node<'_>) -> Result<Mat4, ImportError> {
    let transform = Mat4::from_cols_array_2d(&node.transform().matrix());
    if !transform.is_finite() {
        return Err(ImportError::Invalid("node transform must be finite"));
    }
    let affine = transform.x_axis.w.abs() <= f32::EPSILON
        && transform.y_axis.w.abs() <= f32::EPSILON
        && transform.z_axis.w.abs() <= f32::EPSILON
        && (transform.w_axis.w - 1.0).abs() <= f32::EPSILON;
    if !affine {
        return Err(ImportError::Unsupported("node matrix must be affine"));
    }
    Ok(transform)
}

fn srgb_to_linear(byte: u8) -> f32 {
    let encoded = f32::from(byte) / 255.0;
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(linear: f32) -> u8 {
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

fn mip_chain_len(mut width: u32, mut height: u32) -> Result<usize, ImportError> {
    let mut total = 0_usize;
    loop {
        let level = width as usize * height as usize * 4;
        total = total
            .checked_add(level)
            .ok_or(ImportError::Capacity("texture mip bytes"))?;
        if width == 1 && height == 1 {
            return Ok(total);
        }
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
}

fn append_mip_levels(
    mut width: u32,
    mut height: u32,
    mut rgba8: Vec<u8>,
) -> Result<Vec<u8>, ImportError> {
    let total = mip_chain_len(width, height)?;
    if total > MAX_TEXTURE_MIP_RGBA_BYTES {
        return Err(ImportError::Capacity("texture mip bytes"));
    }
    rgba8
        .try_reserve_exact(total - rgba8.len())
        .map_err(|_| ImportError::Capacity("texture mip allocation"))?;
    let mut source_offset = 0_usize;

    while width > 1 || height > 1 {
        let next_width = (width / 2).max(1);
        let next_height = (height / 2).max(1);
        for y in 0..next_height {
            let source_y_start = y * height / next_height;
            let source_y_end = (y + 1) * height / next_height;
            for x in 0..next_width {
                let source_x_start = x * width / next_width;
                let source_x_end = (x + 1) * width / next_width;
                let mut linear_rgb = [0.0_f32; 3];
                let mut alpha = 0.0_f32;
                let mut samples = 0_u32;
                for source_y in source_y_start..source_y_end {
                    for source_x in source_x_start..source_x_end {
                        let pixel = source_offset + ((source_y * width + source_x) * 4) as usize;
                        for channel in 0..3 {
                            linear_rgb[channel] += srgb_to_linear(rgba8[pixel + channel]);
                        }
                        alpha += f32::from(rgba8[pixel + 3]);
                        samples += 1;
                    }
                }
                let divisor = samples as f32;
                rgba8.extend(linear_rgb.map(|channel| linear_to_srgb(channel / divisor)));
                rgba8.push((alpha / divisor).round() as u8);
            }
        }
        source_offset += (width * height * 4) as usize;
        width = next_width;
        height = next_height;
    }
    debug_assert_eq!(rgba8.len(), total);
    Ok(rgba8)
}

fn decode_base_color_png(encoded: &[u8]) -> Result<BaseColorTexture, ImportError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(encoded));
    decoder.set_limits(png::Limits {
        bytes: MAX_TEXTURE_RGBA_BYTES,
    });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().map_err(ImportError::Image)?;
    let header = reader.info();
    if header.width == 0
        || header.height == 0
        || header.width > MAX_TEXTURE_DIMENSION
        || header.height > MAX_TEXTURE_DIMENSION
    {
        return Err(ImportError::Capacity("texture dimensions"));
    }
    if header.animation_control.is_some() {
        return Err(ImportError::Unsupported("animated PNG is deferred"));
    }
    let pixels = (header.width as usize)
        .checked_mul(header.height as usize)
        .ok_or(ImportError::Capacity("texture pixels"))?;
    if pixels > MAX_TEXTURE_PIXELS {
        return Err(ImportError::Capacity("texture pixels"));
    }
    let expected = pixels * 4;
    let output = reader
        .output_buffer_size()
        .ok_or(ImportError::Capacity("decoded texture bytes"))?;
    if output != expected || output > MAX_TEXTURE_RGBA_BYTES {
        return Err(ImportError::Unsupported("base-colour PNG must be RGBA8"));
    }
    let mut rgba8 = vec![0; output];
    let info = reader.next_frame(&mut rgba8).map_err(ImportError::Image)?;
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || info.buffer_size() != expected
    {
        return Err(ImportError::Unsupported("base-colour PNG must be RGBA8"));
    }
    let rgba8_mip_chain = append_mip_levels(info.width, info.height, rgba8)?;
    Ok(BaseColorTexture {
        width: info.width,
        height: info.height,
        rgba8_mip_chain,
    })
}

pub(crate) struct ImportedMaterial<'a> {
    pub(crate) base_color_factor: [f32; 4],
    pub(crate) base_color_texture: BaseColorTexture,
    pub(crate) image_view: gltf::buffer::View<'a>,
}

pub(crate) fn import_material<'a>(
    primitive: &gltf::Primitive<'a>,
    blob: &[u8],
) -> Result<ImportedMaterial<'a>, ImportError> {
    let material = primitive.material();
    if material.index() != Some(0) {
        return Err(ImportError::Invalid(
            "the primitive must reference the sole material",
        ));
    }
    if material.alpha_mode() != gltf::material::AlphaMode::Opaque
        || material.alpha_cutoff().is_some()
        || material.double_sided()
        || material.normal_texture().is_some()
        || material.occlusion_texture().is_some()
        || material.emissive_texture().is_some()
        || material.emissive_factor() != [0.0; 3]
    {
        return Err(ImportError::Unsupported(
            "only an opaque, single-sided base-colour material is accepted",
        ));
    }
    let pbr = material.pbr_metallic_roughness();
    let pbr_scalars = [pbr.metallic_factor(), pbr.roughness_factor()];
    if pbr_scalars
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(ImportError::Invalid(
            "metallic and roughness factors must be finite and within zero to one",
        ));
    }
    if pbr.metallic_roughness_texture().is_some() {
        return Err(ImportError::Unsupported(
            "metallic-roughness textures are deferred",
        ));
    }
    let base_color_factor = pbr.base_color_factor();
    if base_color_factor
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(ImportError::Invalid(
            "baseColorFactor components must be finite and within zero to one",
        ));
    }
    let base_color = pbr
        .base_color_texture()
        .ok_or(ImportError::Invalid("baseColorTexture is required"))?;
    if base_color.tex_coord() != 0 || base_color.texture().index() != 0 {
        return Err(ImportError::Unsupported(
            "baseColorTexture must use texture zero and TEXCOORD_0",
        ));
    }
    let texture = base_color.texture();
    let sampler = texture.sampler();
    if sampler.index() != Some(0)
        || sampler.mag_filter() != Some(gltf::texture::MagFilter::Linear)
        || sampler.min_filter() != Some(gltf::texture::MinFilter::LinearMipmapLinear)
        || sampler.wrap_s() != gltf::texture::WrappingMode::Repeat
        || sampler.wrap_t() != gltf::texture::WrappingMode::Repeat
    {
        return Err(ImportError::Unsupported(
            "the sole sampler must use linear magnification, trilinear minification, and repeat wrapping",
        ));
    }
    let image = texture.source();
    if image.index() != 0 {
        return Err(ImportError::Invalid("texture references the wrong image"));
    }
    let image_view = match image.source() {
        gltf::image::Source::View {
            view,
            mime_type: "image/png",
        } => view,
        gltf::image::Source::View { .. } => {
            return Err(ImportError::Unsupported(
                "base-colour image must declare image/png",
            ));
        }
        gltf::image::Source::Uri { .. } => {
            return Err(ImportError::Unsupported(
                "external and data URI images are rejected",
            ));
        }
    };
    if image_view.buffer().index() != 0 {
        return Err(ImportError::Invalid(
            "base-colour image must use the embedded buffer",
        ));
    }
    let image_end = image_view
        .offset()
        .checked_add(image_view.length())
        .ok_or(ImportError::Invalid("base-colour image range overflows"))?;
    let encoded_image = blob
        .get(image_view.offset()..image_end)
        .ok_or(ImportError::Invalid(
            "base-colour image exceeds the embedded buffer",
        ))?;
    let base_color_texture = decode_base_color_png(encoded_image)?;
    Ok(ImportedMaterial {
        base_color_factor,
        base_color_texture,
        image_view,
    })
}

/// Imports the deliberately small Stage 2 subset of binary glTF 2.0.
///
/// Only one embedded-buffer, indexed, static triangle mesh is accepted. The
/// selected node chain is collapsed into the returned vertices exactly once;
/// runtime code therefore needs no per-asset axis or root correction.
pub fn import_glb(bytes: &[u8]) -> Result<StaticMesh, ImportError> {
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
            "extensions are not accepted in Stage 2",
        ));
    }
    exactly(
        document.scenes().count(),
        1,
        "exactly one scene is required",
    )?;
    exactly(document.meshes().count(), 1, "exactly one mesh is required")?;
    exactly(
        document.buffers().count(),
        1,
        "exactly one embedded buffer is required",
    )?;
    exactly(
        document.accessors().count(),
        4,
        "exactly four accessors are required",
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

    if document.animations().next().is_some()
        || document.cameras().next().is_some()
        || document.skins().next().is_some()
    {
        return Err(ImportError::Unsupported(
            "cameras, skins, and animation are deferred",
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

    let scene = document.default_scene().ok_or(ImportError::Unsupported(
        "the sole scene must be selected as the default",
    ))?;
    exactly(
        scene.nodes().count(),
        1,
        "the scene must have one root node",
    )?;
    let mut node = scene.nodes().next().expect("count checked");
    let mut transform = Mat4::IDENTITY;
    let mut visited = 0_usize;
    let primitive;

    loop {
        visited += 1;
        transform *= node_transform(node.clone())?;
        if node.camera().is_some() || node.skin().is_some() {
            return Err(ImportError::Unsupported(
                "preview nodes cannot contain cameras or skins",
            ));
        }

        if let Some(mesh) = node.mesh() {
            if mesh.index() != 0 {
                return Err(ImportError::Invalid("scene references the wrong mesh"));
            }
            if node.children().next().is_some() {
                return Err(ImportError::Unsupported(
                    "only transform ancestors may accompany the mesh node",
                ));
            }
            exactly(
                mesh.primitives().count(),
                1,
                "exactly one mesh primitive is required",
            )?;
            primitive = mesh.primitives().next().expect("count checked");
            break;
        }

        exactly(
            node.children().count(),
            1,
            "transform nodes must form one chain",
        )?;
        node = node.children().next().expect("count checked");
    }

    exactly(
        document.nodes().count(),
        visited,
        "unused nodes are rejected",
    )?;
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
        3,
        "only POSITION, NORMAL and TEXCOORD_0 vertex attributes are accepted",
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
    let indices = primitive
        .indices()
        .ok_or(ImportError::Unsupported("indices are required"))?;
    let mut used_views = [positions.view(), normals.view(), uvs.view(), indices.view()]
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
    if positions.count() == 0
        || positions.count() != normals.count()
        || positions.count() != uvs.count()
    {
        return Err(ImportError::Invalid(
            "POSITION, NORMAL and TEXCOORD_0 counts must be equal and nonzero",
        ));
    }
    if uvs.data_type() != gltf::accessor::DataType::F32
        || uvs.dimensions() != gltf::accessor::Dimensions::Vec2
        || uvs.normalized()
    {
        return Err(ImportError::Unsupported(
            "TEXCOORD_0 must be unnormalized FLOAT VEC2",
        ));
    }
    if positions.count() > MAX_VERTICES {
        return Err(ImportError::Capacity("vertex count"));
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
    let linear = Mat3::from_mat4(transform);
    let determinant = linear.determinant();
    if !determinant.is_finite() || determinant.abs() <= 1.0e-8 {
        return Err(ImportError::Invalid("node transform must be invertible"));
    }
    let normal_transform = linear.inverse().transpose();
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
    let mut vertices = Vec::with_capacity(positions.count());
    for ((position, normal), uv) in source_positions.zip(source_normals).zip(source_uvs) {
        let position = transform.transform_point3(Vec3::from(position));
        let source_normal = Vec3::from(normal);
        let uv = Vec2::from(uv);
        if !position.is_finite()
            || !source_normal.is_finite()
            || !uv.is_finite()
            || (source_normal.length_squared() - 1.0).abs() > 1.0e-3
        {
            return Err(ImportError::Invalid(
                "positions and UVs must be finite and normals unit length",
            ));
        }
        let normal = normal_transform * source_normal;
        if !normal.is_finite() || normal.length_squared() <= 1.0e-12 {
            return Err(ImportError::Invalid("transformed normal is invalid"));
        }
        vertices.push(Vertex {
            position: position.into(),
            normal: normal.normalize().into(),
            uv: uv.into(),
        });
    }
    if vertices.len() != positions.count() {
        return Err(ImportError::Invalid("vertex accessor ended early"));
    }

    let mut imported_indices: Vec<u32> = reader
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
    if determinant.is_sign_negative() {
        let (triangles, remainder) = imported_indices.as_chunks_mut::<3>();
        debug_assert!(remainder.is_empty(), "triangle count was validated");
        for triangle in triangles {
            triangle.swap(1, 2);
        }
    }

    Ok(StaticMesh {
        vertices,
        indices: imported_indices,
        base_color_factor: material.base_color_factor,
        base_color_texture: material.base_color_texture,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../../assets/fixtures/static-preview.glb");
    const BLENDER_FIXTURE: &[u8] =
        include_bytes!("../../../assets/fixtures/blender-static-preview.glb");

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
    fn imports_the_asymmetric_fixture() {
        let mesh = import_glb(FIXTURE).unwrap();
        assert_eq!((mesh.vertex_count(), mesh.index_count()), (12, 12));
        assert_eq!(mesh.base_color_factor(), [0.25, 1.0, 1.0, 1.0]);
        assert_eq!(
            (
                mesh.base_color_texture().width(),
                mesh.base_color_texture().height()
            ),
            (4, 4)
        );
        let pixels = mesh.base_color_texture().rgba8();
        assert_eq!(&pixels[0..4], &[128, 128, 0, 255]);
        assert_eq!(&pixels[12..16], &[0, 255, 255, 255]);
        assert_eq!(&pixels[32..36], &[255, 0, 255, 255]);
        assert_eq!(&pixels[60..64], &[255, 255, 255, 255]);
        assert!(mesh
            .vertices()
            .iter()
            .any(|vertex| vertex.position() == Vec3::new(0.0, 0.0, 1.0)));
        assert_eq!(mesh.vertices()[0].uv(), Vec2::new(0.125, 0.125));
        assert_eq!(mesh.vertices()[11].uv(), Vec2::new(0.125, 0.875));
        assert!(mesh
            .vertices()
            .iter()
            .all(|vertex| (vertex.normal().length() - 1.0).abs() < 1.0e-5));
        assert_eq!(mesh.base_color_texture().mip_level_count(), 3);
        assert_eq!(mesh.base_color_texture().rgba8_mip_chain().len(), 84);
    }

    #[test]
    fn imports_the_blender_authored_fixture() {
        let mesh = import_glb(BLENDER_FIXTURE).unwrap();
        assert_eq!((mesh.vertex_count(), mesh.index_count()), (144, 216));
        assert_eq!(mesh.base_color_factor(), [1.0; 4]);
        let texture = mesh.base_color_texture();
        assert_eq!((texture.width(), texture.height()), (16, 16));
        assert_eq!(texture.mip_level_count(), 5);
        assert_eq!(texture.rgba8_mip_chain().len(), 1_364);

        let (min_y, max_y, max_z) = mesh.vertices().iter().fold(
            (f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
            |(min_y, max_y, max_z), vertex| {
                let position = vertex.position();
                (
                    min_y.min(position.y),
                    max_y.max(position.y),
                    max_z.max(position.z),
                )
            },
        );
        assert!(
            (0.03..=0.05).contains(&min_y),
            "feet are not grounded: {min_y}"
        );
        assert!(
            (1.88..=1.90).contains(&max_y),
            "metre scale changed: {max_y}"
        );
        assert!(max_z > 0.2, "+Z-facing asymmetry was lost: {max_z}");
    }

    #[test]
    fn mip_generation_averages_base_colour_in_linear_space() {
        let black = [0, 0, 0, 255];
        let white = [255, 255, 255, 255];
        let base = [black, white, white, black].concat();
        let chain = append_mip_levels(2, 2, base).unwrap();
        assert_eq!(&chain[16..], &[188, 188, 188, 255]);
    }

    #[test]
    fn node_transforms_are_applied_once_at_import() {
        let moved = mutate_json(
            FIXTURE,
            r#"{"mesh":0}"#,
            r#"{"mesh":0,"translation":[2,0,0]}"#,
        );
        let mesh = import_glb(&moved).unwrap();
        assert!(mesh
            .vertices()
            .iter()
            .any(|vertex| vertex.position() == Vec3::new(2.0, 0.0, 1.0)));

        let reflected = mutate_json(FIXTURE, r#"{"mesh":0}"#, r#"{"mesh":0,"scale":[-1,1,1]}"#);
        assert_eq!(&import_glb(&reflected).unwrap().indices()[..3], &[0, 2, 1]);
    }

    #[test]
    fn the_format_boundary_rejects_unsupported_valid_gltf() {
        let cases = [
            mutate_json(FIXTURE, r#""mode":4"#, r#""mode":1"#),
            mutate_json(
                FIXTURE,
                r#""componentType":5123"#,
                r#""componentType":5121"#,
            ),
            mutate_json(FIXTURE, r#""NORMAL":1"#, r#""_NORMAL":1"#),
            mutate_json(FIXTURE, r#""TEXCOORD_0":2"#, r#""_TEXCOORD_0":2"#),
            mutate_json(
                FIXTURE,
                r#""buffers":[{"byteLength":544}]"#,
                r#""buffers":[{"byteLength":544,"uri":"mesh.bin"}]"#,
            ),
            mutate_json(FIXTURE, r#""texCoord":0"#, r#""texCoord":1"#),
            mutate_json(FIXTURE, r#""magFilter":9729"#, r#""magFilter":9728"#),
            mutate_json(FIXTURE, r#""minFilter":9987"#, r#""minFilter":9729"#),
            mutate_json(FIXTURE, r#""image/png""#, r#""image/jpeg""#),
            mutate_json(
                FIXTURE,
                r#"{"bufferView":4,"mimeType":"image/png"}"#,
                r#"{"uri":"texture.png","mimeType":"image/png"}"#,
            ),
            mutate_json(
                FIXTURE,
                r#""materials":[{"pbr"#,
                r#""materials":[{"normalTexture":{"index":0},"pbr"#,
            ),
            mutate_json(FIXTURE, r#""scene":0,"#, ""),
            mutate_json(
                FIXTURE,
                r#""scenes":[{"nodes":[0]}]"#,
                r#""scenes":[{"nodes":[0]},{"nodes":[0]}]"#,
            ),
            mutate_json(
                FIXTURE,
                r#""nodes":[{"mesh":0}]"#,
                r#""nodes":[{"mesh":0},{}]"#,
            ),
        ];
        for bytes in cases {
            assert!(import_glb(&bytes).is_err());
        }
    }

    #[test]
    fn invalid_transforms_and_unbounded_files_are_rejected() {
        let singular = mutate_json(FIXTURE, r#"{"mesh":0}"#, r#"{"mesh":0,"scale":[0,1,1]}"#);
        assert!(import_glb(&singular)
            .unwrap_err()
            .to_string()
            .contains("invertible"));
        assert!(import_glb(&vec![0; MAX_GLB_BYTES + 1])
            .unwrap_err()
            .to_string()
            .contains("file bytes"));
        assert!(import_glb(b"{}")
            .unwrap_err()
            .to_string()
            .contains("binary .glb"));

        let too_many_vertices = mutate_json(FIXTURE, r#""count":12"#, r#""count":262146"#);
        let error = import_glb(&too_many_vertices).unwrap_err().to_string();
        assert!(error.contains("vertex count"), "unexpected error: {error}");
    }

    #[test]
    fn decoded_texture_dimensions_are_bounded_before_pixel_allocation() {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, MAX_TEXTURE_DIMENSION + 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&vec![0; (MAX_TEXTURE_DIMENSION as usize + 1) * 4])
                .unwrap();
        }
        assert!(decode_base_color_png(&png)
            .unwrap_err()
            .to_string()
            .contains("texture dimensions"));
    }
}
