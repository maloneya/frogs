struct Camera {
    view_proj: mat4x4<f32>,
};

struct Material {
    base_color_factor: vec4<f32>,
};

struct JointPalette {
    transforms: array<mat4x4<f32>, 64>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var base_color_sampler: sampler;
@group(1) @binding(2) var<uniform> material: Material;
@group(2) @binding(0) var<uniform> joints: JointPalette;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(5) uv: vec2<f32>,
    @location(6) joint: vec4<u32>,
    @location(7) weight: vec4<f32>,
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
    let skin =
        joints.transforms[in.joint.x] * in.weight.x +
        joints.transforms[in.joint.y] * in.weight.y +
        joints.transforms[in.joint.z] * in.weight.z +
        joints.transforms[in.joint.w] * in.weight.w;
    let skinned_pos = (skin * vec4<f32>(in.pos, 1.0)).xyz;
    let skinned_normal = (skin * vec4<f32>(in.normal, 0.0)).xyz;
    let yaw = in.i_pos.w;
    let world_pos = rotate_y(skinned_pos * in.i_scale.xyz, yaw) + in.i_pos.xyz;
    out.clip = camera.view_proj * vec4<f32>(world_pos, 1.0);
    let normal = normalize(rotate_y(skinned_normal / in.i_scale.xyz, yaw));
    let light_dir = normalize(vec3<f32>(0.4, 1.0, 0.3));
    out.shade = dot(normal, light_dir) * 0.5 + 0.5;
    out.tint = in.i_color.xyz;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(base_color_texture, base_color_sampler, in.uv);
    let linear = texel.rgb * material.base_color_factor.rgb * in.tint * in.shade;
    return vec4<f32>(linear, 1.0);
}
