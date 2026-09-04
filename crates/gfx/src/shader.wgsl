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
    //
    // `i_pos.w` is yaw, riding in what used to be padding — which is why adding
    // rotation cost no layout change at all.
    @location(2) i_pos: vec4<f32>,
    @location(3) i_scale: vec4<f32>,
    @location(4) i_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) shade: f32,
    @location(1) color: vec3<f32>,
};

// Rotation about the vertical axis.
//
// The convention is fixed by `Instance::with_yaw` on the Rust side and has to
// match it exactly: yaw 0 faces +Z, positive yaw turns toward +X. Check by
// substituting the local +Z axis (0, 0, 1), which comes out as (sin, 0, cos) —
// the direction `atan2(dir.x, dir.z)` inverts.
fn rotate_y(v: vec3<f32>, yaw: f32) -> vec3<f32> {
    let s = sin(yaw);
    let c = cos(yaw);
    return vec3<f32>(v.x * c + v.z * s, v.y, -v.x * s + v.z * c);
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;

    let yaw = in.i_pos.w;

    // Scale, *then* rotate, then translate. The other order shears the mesh as
    // it turns: a non-uniform scale applied after a rotation stretches along
    // world axes rather than the body's own, so a character that is deeper than
    // it is wide would smear into a rhombus at 45° and snap back at 90°.
    let world_pos = rotate_y(in.pos * in.i_scale.xyz, yaw) + in.i_pos.xyz;
    out.clip = camera.view_proj * vec4<f32>(world_pos, 1.0);

    // Non-uniform scale skews normals: a squashed cube's side faces no longer
    // point where the unscaled normal says. Dividing by the scale is the
    // inverse-transpose, which for an axis-aligned scale is just this. The
    // rotation then applies unchanged — a rotation is orthonormal, so it is its
    // own inverse-transpose — but it must come *after* the divide, in the same
    // order as the position above.
    let normal = normalize(rotate_y(in.normal / in.i_scale.xyz, yaw));

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
