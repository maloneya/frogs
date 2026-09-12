struct Camera {
    view_proj: mat4x4<f32>,
};

struct Material {
    base_color_factor: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var base_color_sampler: sampler;
@group(1) @binding(2) var<uniform> material: Material;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Locations 2–4 retain the shared Instance layout; imported UVs use the
    // next free slot so the cube pipeline does not acquire an asset concern.
    @location(5) uv: vec2<f32>,
    @location(2) i_pos: vec4<f32>,
    @location(3) i_scale: vec4<f32>,
    @location(4) i_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) shade: f32,
    @location(1) tint: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

fn rotate_y(v: vec3<f32>, yaw: f32) -> vec3<f32> {
    let s = sin(yaw);
    let c = cos(yaw);
    return vec3<f32>(v.x * c + v.z * s, v.y, -v.x * s + v.z * c);
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let yaw = in.i_pos.w;
    let world_pos = rotate_y(in.pos * in.i_scale.xyz, yaw) + in.i_pos.xyz;
    out.clip = camera.view_proj * vec4<f32>(world_pos, 1.0);
    let normal = normalize(rotate_y(in.normal / in.i_scale.xyz, yaw));
    let light_dir = normalize(vec3<f32>(0.4, 1.0, 0.3));
    out.shade = dot(normal, light_dir) * 0.5 + 0.5;
    out.tint = in.i_color.xyz;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Rgba8UnormSrgb decoding happens before this multiplication. The surface
    // performs the matching linear-to-sRGB encode on output.
    let texel = textureSample(base_color_texture, base_color_sampler, in.uv);
    let linear = texel.rgb * material.base_color_factor.rgb * in.tint * in.shade;
    return vec4<f32>(linear, 1.0);
}
