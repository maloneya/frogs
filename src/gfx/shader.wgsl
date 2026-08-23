struct Camera {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) shade: f32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = camera.view_proj * vec4<f32>(in.pos, 1.0);

    // Half-lambert: remap dot from [-1, 1] to [0, 1] so faces angled away are
    // dim rather than black. Under an isometric camera exactly three faces are
    // ever visible, so three distinct shades is all it takes to read as solid.
    let light_dir = normalize(vec3<f32>(0.4, 1.0, 0.3));
    out.shade = dot(in.normal, light_dir) * 0.5 + 0.5;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base = vec3<f32>(0.55, 0.60, 0.72);
    return vec4<f32>(base * in.shade, 1.0);
}
