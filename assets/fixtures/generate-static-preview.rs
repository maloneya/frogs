//! Rebuilds `static-preview.glb` using only the Rust standard library.
//!
//! Run from the repository root:
//!
//! ```text
//! rustc assets/fixtures/generate-static-preview.rs -o /tmp/generate-static-preview
//! /tmp/generate-static-preview
//! ```

use std::io::Write as _;

fn push_f32s(out: &mut Vec<u8>, values: &[[f32; 3]]) {
    for value in values {
        for component in value {
            out.extend_from_slice(&component.to_le_bytes());
        }
    }
}

fn push_uvs(out: &mut Vec<u8>, values: &[[f32; 2]]) {
    for value in values {
        for component in value {
            out.extend_from_slice(&component.to_le_bytes());
        }
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut checked = Vec::with_capacity(kind.len() + data.len());
    checked.extend_from_slice(kind);
    checked.extend_from_slice(data);
    out.extend_from_slice(&crc32(&checked).to_be_bytes());
}

fn diagnostic_png() -> Vec<u8> {
    const WIDTH: u32 = 4;
    const HEIGHT: u32 = 4;
    // glTF's UV origin is the upper-left. Each texel is intentionally unique;
    // the half-red base-colour factor makes transfer-function mistakes visible.
    let mut scanlines = Vec::new();
    for y in 0..HEIGHT {
        scanlines.push(0); // PNG filter: None.
        for x in 0..WIDTH {
            let pixel = match (x < WIDTH / 2, y < HEIGHT / 2) {
                (true, true) => [128, 128, 0, 255],
                (false, true) => [0, 255, 255, 255],
                (true, false) => [255, 0, 255, 255],
                (false, false) => [255, 255, 255, 255],
            };
            scanlines.extend_from_slice(&pixel);
        }
    }
    let mut zlib = vec![0x78, 0x01, 0x01]; // Deflate, one final stored block.
    let len = scanlines.len() as u16;
    zlib.extend_from_slice(&len.to_le_bytes());
    zlib.extend_from_slice(&(!len).to_le_bytes());
    zlib.extend_from_slice(&scanlines);
    let mut a = 1_u32;
    let mut b = 0_u32;
    for &byte in &scanlines {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&WIDTH.to_be_bytes());
    header.extend_from_slice(&HEIGHT.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // RGBA8, no interlace.
    png_chunk(&mut png, b"IHDR", &header);
    png_chunk(&mut png, b"IDAT", &zlib);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

fn main() {
    // A tetrahedral pointer: its long ground-plane axis points along glTF +Z,
    // while its apex makes vertical scale and winding visible.
    let a = [-0.5, 0.0, -0.5];
    let b = [0.5, 0.0, -0.5];
    let c = [0.0, 0.0, 1.0];
    let d = [0.0, 1.0, 0.0];
    let positions = [a, b, c, a, d, b, b, d, c, c, d, a];
    let normals = [
        [0.0, -1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.447_213_6, -0.894_427_2],
        [0.0, 0.447_213_6, -0.894_427_2],
        [0.0, 0.447_213_6, -0.894_427_2],
        [0.904_534, 0.301_511_35, 0.301_511_35],
        [0.904_534, 0.301_511_35, 0.301_511_35],
        [0.904_534, 0.301_511_35, 0.301_511_35],
        [-0.904_534, 0.301_511_35, 0.301_511_35],
        [-0.904_534, 0.301_511_35, 0.301_511_35],
        [-0.904_534, 0.301_511_35, 0.301_511_35],
    ];
    // Every face maps the upper-left texture triangle. The visible face then
    // contains three distinct corners, so orientation and interpolation are
    // observable without an importer or shader-side V flip.
    let uvs = [
        [0.125, 0.125],
        [0.875, 0.125],
        [0.125, 0.875],
        [0.125, 0.125],
        [0.125, 0.875],
        [0.875, 0.125],
        [0.125, 0.125],
        [0.875, 0.125],
        [0.125, 0.875],
        [0.125, 0.125],
        [0.875, 0.125],
        [0.125, 0.875],
    ];

    let mut bin = Vec::new();
    push_f32s(&mut bin, &positions);
    push_f32s(&mut bin, &normals);
    push_uvs(&mut bin, &uvs);
    for index in 0_u16..12 {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    assert_eq!(bin.len(), 408);
    let image_offset = bin.len();
    let png = diagnostic_png();
    bin.extend_from_slice(&png);
    let buffer_length = bin.len();

    let mut json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"arpg fixture generator\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[0]}}],\"nodes\":[{{\"mesh\":0}}],\"meshes\":[{{\"primitives\":[{{\"attributes\":{{\"POSITION\":0,\"NORMAL\":1,\"TEXCOORD_0\":2}},\"indices\":3,\"material\":0,\"mode\":4}}]}}],\"materials\":[{{\"pbrMetallicRoughness\":{{\"baseColorFactor\":[0.25,1.0,1.0,1.0],\"baseColorTexture\":{{\"index\":0,\"texCoord\":0}}}}}}],\"textures\":[{{\"sampler\":0,\"source\":0}}],\"samplers\":[{{\"magFilter\":9729,\"minFilter\":9987,\"wrapS\":10497,\"wrapT\":10497}}],\"images\":[{{\"bufferView\":4,\"mimeType\":\"image/png\"}}],\"buffers\":[{{\"byteLength\":{buffer_length}}}],\"bufferViews\":[{{\"buffer\":0,\"byteOffset\":0,\"byteLength\":144,\"target\":34962}},{{\"buffer\":0,\"byteOffset\":144,\"byteLength\":144,\"target\":34962}},{{\"buffer\":0,\"byteOffset\":288,\"byteLength\":96,\"target\":34962}},{{\"buffer\":0,\"byteOffset\":384,\"byteLength\":24,\"target\":34963}},{{\"buffer\":0,\"byteOffset\":{image_offset},\"byteLength\":{}}}],\"accessors\":[{{\"bufferView\":0,\"componentType\":5126,\"count\":12,\"type\":\"VEC3\",\"min\":[-0.5,0.0,-0.5],\"max\":[0.5,1.0,1.0]}},{{\"bufferView\":1,\"componentType\":5126,\"count\":12,\"type\":\"VEC3\"}},{{\"bufferView\":2,\"componentType\":5126,\"count\":12,\"type\":\"VEC2\"}},{{\"bufferView\":3,\"componentType\":5123,\"count\":12,\"type\":\"SCALAR\",\"min\":[0],\"max\":[11]}}]}}",
        png.len()
    )
    .into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }

    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2_u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4e4f_534a_u32.to_le_bytes());
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004e_4942_u32.to_le_bytes());
    glb.extend_from_slice(&bin);

    let path = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| "assets/fixtures/static-preview.glb".into());
    let mut file = std::fs::File::create(&path).expect("create fixture");
    file.write_all(&glb).expect("write fixture");
}
