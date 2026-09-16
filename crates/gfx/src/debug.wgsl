struct Camera { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
};
@vertex
fn vs_main(@location(0) point: vec2<f32>, @location(1) centre_radius: vec4<f32>, @location(2) colour: vec4<f32>) -> VertexOut {
    var out: VertexOut;
    let world = centre_radius.xyz + vec3<f32>(point.x, 0.0, point.y) * centre_radius.w;
    out.position = camera.view_proj * vec4<f32>(world, 1.0);
    out.colour = colour;
    return out;
}
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> { return in.colour; }
