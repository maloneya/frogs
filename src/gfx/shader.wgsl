struct Camera {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct VsIn {
    // Per-vertex: advances once per vertex.
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Per-instance: advances once per instance. Same buffer machinery, a
    // different stepping rule.
    @location(2) i_pos: vec4<f32>,
    @location(3) i_scale: vec4<f32>,
    @location(4) i_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) shade: f32,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;

    let world_pos = in.pos * in.i_scale.xyz + in.i_pos.xyz;
    out.clip = camera.view_proj * vec4<f32>(world_pos, 1.0);

    // Non-uniform scale skews normals: a squashed cube's side faces no longer
    // point where the unscaled normal says. Dividing by the scale is the
    // inverse-transpose, which for an axis-aligned scale is just this.
    let normal = normalize(in.normal / in.i_scale.xyz);

    // Half-lambert: remap dot from [-1, 1] to [0, 1] so faces angled away are
    // dim rather than black. Under an isometric camera exactly three faces are
    // ever visible, so three distinct shades is all it takes to read as solid.
    let light_dir = normalize(vec3<f32>(0.4, 1.0, 0.3));
    out.shade = dot(normal, light_dir) * 0.5 + 0.5;
    out.color = in.i_color.xyz;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color * in.shade, 1.0);
}
