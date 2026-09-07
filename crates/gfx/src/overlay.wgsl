// The overlay: screen-space quads drawn over the finished world.
//
// Deliberately the simplest shader in the engine. There is no camera here — a
// quad is already in pixels — so the only transform is pixels to clip space,
// and the only shading is a texture fetch times a colour.

struct Screen {
    // Physical pixels. `zw` is padding: a uniform buffer's size must round up
    // to 16 bytes, and naming the padding beats letting the layout rules
    // silently insert it somewhere the Rust side did not expect.
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct VsIn {
    // Which corner of the quad this is. There is no vertex buffer: six
    // vertices per instance are generated from this index alone, which is why
    // the overlay needs no mesh, no index buffer and no upload but its own.
    @builtin(vertex_index) corner: u32,
    // Per-instance, matching `arpg_core::Quad` field for field.
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

// The two triangles of a quad, as unit-square corners. Counter-clockwise in a
// y-down space, which is clockwise once y is flipped into clip space — so this
// pipeline does not cull, and does not need to: an overlay quad is never seen
// from behind.
fn unit_corner(index: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    return corners[index];
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;

    let corner = unit_corner(in.corner);
    let pixel = in.rect.xy + corner * in.rect.zw;

    // Pixels to clip space. The y term is subtracted rather than added because
    // the overlay's origin is top-left and y grows downward — winit's
    // convention, kept so a cursor position can be compared against a quad
    // without a conversion in between. Getting this backwards draws everything
    // mirrored about the horizon, which is obvious the first time and worth
    // saying anyway because the fix is a single character.
    out.clip = vec4<f32>(
        pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.size.y * 2.0,
        0.0,
        1.0,
    );

    out.uv = mix(in.uv.xy, in.uv.zw, corner);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // The atlas is one channel of coverage, so it modulates alpha and never
    // colour. That is what lets a single white glyph be drawn in any colour,
    // and what makes a solid quad — whose UVs collapse onto an opaque texel —
    // come out at exactly the colour it asked for.
    let coverage = textureSample(atlas, atlas_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
