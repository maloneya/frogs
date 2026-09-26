struct PointLight {
    position_range: vec4<f32>,
    colour_intensity: vec4<f32>,
};
struct Lights {
    count: vec4<u32>,
    ambient: vec4<f32>,
    directional: vec4<f32>,
    direction: vec4<f32>,
    lights: array<PointLight, MAX_POINT_LIGHTS>,
};
@group(0) @binding(1) var<uniform> scene_lights: Lights;

fn contribution(light: PointLight, world_pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let offset = light.position_range.xyz - world_pos;
    let distance_squared = dot(offset, offset);
    // Softened inverse-square falloff avoids a singularity at the source.
    // The squared range fade reaches black smoothly, without a hard ring.
    let range = light.position_range.w;
    let fade = max(1.0 - distance_squared / max(range * range, 0.0001), 0.0);
    let direction = offset * inverseSqrt(max(distance_squared, 0.0001));
    let facing = max(dot(normal, direction), 0.0);
    return light.colour_intensity.rgb * light.colour_intensity.w
        * facing * fade * fade / (1.0 + distance_squared);
}

fn illumination(world_pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let unit_normal = normalize(normal);
    let facing = max(dot(unit_normal, scene_lights.direction.xyz), 0.0);
    var total = scene_lights.ambient.rgb * scene_lights.ambient.w
        + scene_lights.directional.rgb * scene_lights.directional.w * facing;
    for (var i = 0u; i < scene_lights.count.x; i += 1u) {
        total += contribution(scene_lights.lights[i], world_pos, unit_normal);
    }
    return total;
}
