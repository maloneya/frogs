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
    @location(0) world_pos: vec3<f32>,
    @location(3) normal: vec3<f32>,
    @location(1) tint: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) @interpolate(flat) white_mix: f32,
};

fn rotate_x(v: vec3<f32>, pitch: f32) -> vec3<f32> {
    let s = sin(pitch);
    let c = cos(pitch);
    return vec3<f32>(v.x, c * v.y - s * v.z, s * v.y + c * v.z);
}

fn rotate_y(v: vec3<f32>, yaw: f32) -> vec3<f32> {
    let s = sin(yaw);
    let c = cos(yaw);
    return vec3<f32>(v.x * c + v.z * s, v.y, -v.x * s + v.z * c);
}

fn world_position(pos: vec3<f32>, i_pos: vec4<f32>, i_scale: vec4<f32>, i_color: vec4<f32>) -> vec3<f32> {
    return rotate_y(rotate_x(pos * i_scale.xyz, i_color.w), i_pos.w) + i_pos.xyz;
}

fn instance_vertex(pos: vec3<f32>, skinned_normal: vec3<f32>, uv: vec2<f32>,
    i_pos: vec4<f32>, i_scale: vec4<f32>, i_color: vec4<f32>) -> VsOut {
    var out: VsOut;
    let yaw = i_pos.w;
    let world_pos = world_position(pos, i_pos, i_scale, i_color);
    out.clip = camera.view_proj * vec4<f32>(world_pos, 1.0);
    let normal = normalize(rotate_y(rotate_x(skinned_normal / i_scale.xyz, i_color.w), yaw));
    out.world_pos = world_pos;
    out.normal = normal;
    out.tint = i_color.xyz;
    out.white_mix = i_scale.w;
    out.uv = uv;
    return out;
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let skin =
        joints.transforms[in.joint.x] * in.weight.x +
        joints.transforms[in.joint.y] * in.weight.y +
        joints.transforms[in.joint.z] * in.weight.z +
        joints.transforms[in.joint.w] * in.weight.w;
    return instance_vertex((skin * vec4<f32>(in.pos, 1.0)).xyz,
        (skin * vec4<f32>(in.normal, 0.0)).xyz, in.uv, in.i_pos, in.i_scale, in.i_color);
}

struct PosedIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(5) uv: vec2<f32>,
    @location(2) i_pos: vec4<f32>,
    @location(3) i_scale: vec4<f32>,
    @location(4) i_color: vec4<f32>,
};

@vertex
fn vs_posed(in: PosedIn) -> VsOut {
    return instance_vertex(in.pos, in.normal, in.uv, in.i_pos, in.i_scale, in.i_color);
}

struct PosedDepthIn {
    @location(0) pos: vec3<f32>,
    @location(2) i_pos: vec4<f32>,
    @location(3) i_scale: vec4<f32>,
    @location(4) i_color: vec4<f32>,
};

@vertex
fn vs_posed_depth(in: PosedDepthIn) -> @builtin(position) vec4<f32> {
    return camera.view_proj * vec4<f32>(world_position(in.pos, in.i_pos, in.i_scale, in.i_color), 1.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(base_color_texture, base_color_sampler, in.uv);
    let linear = surface_colour(texel.rgb * material.base_color_factor.rgb * in.tint, in.world_pos, in.normal);
    return vec4<f32>(mix(linear, vec3<f32>(1.0), in.white_mix), 1.0);
}
