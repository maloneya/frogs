struct Camera { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
};
@vertex
fn vs_main(@location(0) point: vec2<f32>, @location(1) centre_shape: vec4<f32>, @location(2) extents_yaw: vec4<f32>, @location(3) colour: vec4<f32>) -> VertexOut {
    var out: VertexOut;
    // The circle mesh includes every diagonal corner. Radial projection to
    // the unit square makes the same line list cover all four straight edges.
    var boundary = point;
    if centre_shape.w == 1.0 { boundary /= max(abs(point.x), abs(point.y)); }
    let local = boundary * extents_yaw.xy;
    let c = cos(extents_yaw.z);
    let s = sin(extents_yaw.z);
    let offset = vec3<f32>(c * local.x + s * local.y, 0.0, -s * local.x + c * local.y);
    out.position = camera.view_proj * vec4<f32>(centre_shape.xyz + offset, 1.0);
    out.colour = colour;
    return out;
}
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> { return in.colour; }
