struct Camera { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) opacity: f32,
};
@vertex
fn vs_main(@builtin(vertex_index) vertex: u32,
    @location(2) position: vec4<f32>, @location(3) scale: vec4<f32>,
    @location(4) colour: vec4<f32>) -> VertexOut {
    let corners = array<vec2<f32>,6>(vec2(-1.0,-1.0),vec2(1.0,-1.0),vec2(1.0,1.0),
        vec2(-1.0,-1.0),vec2(1.0,1.0),vec2(-1.0,1.0));
    let uv = corners[vertex];
    // Contact grounding, not a replacement for occluder shadow maps. Point
    // lights pull the soft footprint away from the flame; lightning contracts it.
    var light_direction = scene_lights.direction.xz;
    var influence = scene_lights.directional.w;
    for (var i=0u; i<scene_lights.count.x; i+=1u) {
        let light = scene_lights.lights[i];
        let offset = position.xyz - light.position_range.xyz;
        let d2 = dot(offset,offset);
        let fade = max(1.0-d2/(light.position_range.w*light.position_range.w),0.0);
        let strength = light.colour_intensity.w*fade*fade/(1.0+d2);
        if strength > influence { influence=strength; light_direction=-offset.xz; }
    }
    let away = -light_direction * inverseSqrt(max(dot(light_direction,light_direction),0.0001));
    let length = 0.22 / (1.0 + scene_lights.directional.w);
    let footprint = uv * scale.xz * vec2(0.48,0.36);
    let world = vec3(position.x + footprint.x + away.x*length*scale.x,
        0.008, position.z + footprint.y + away.y*length*scale.z);
    var out: VertexOut;
    out.clip = camera.view_proj * vec4(world,1.0);
    out.uv = uv;
    out.opacity = 0.48 / (1.0+max(position.y,0.0)*2.0);
    return out;
}
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let radius = dot(in.uv,in.uv);
    let alpha = in.opacity * (1.0-smoothstep(0.08,1.0,radius));
    return vec4(0.0,0.0,0.0,alpha);
}
