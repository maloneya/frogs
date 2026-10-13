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

struct Shadows {
    matrices: array<mat4x4<f32>,2>,
    enabled: vec4<u32>,
    filter_scale: vec4<f32>,
};
@group(0) @binding(2) var<uniform> shadows: Shadows;
@group(0) @binding(3) var shadow_depth: texture_depth_2d_array;
@group(0) @binding(4) var shadow_sampler: sampler_comparison;

fn visibility(layer: u32, position: vec3<f32>, normal: vec3<f32>) -> f32 {
    if shadows.enabled[layer] == 0u { return 1.0; }
    var toward_light = scene_lights.direction.xyz;
    var radius = 0.045;
    if layer == 1u {
        let offset = scene_lights.lights[shadows.enabled.z].position_range.xyz-position;
        toward_light = offset*inverseSqrt(max(dot(offset,offset),0.0001));
        radius = clamp(0.035+length(offset.xz)*0.025,0.035,0.30);
    }
    let receiver_bias = 0.006+0.012*(1.0-max(dot(normal,toward_light),0.0));
    let projected = shadows.matrices[layer] * vec4(position+normal*receiver_bias,1.0);
    if projected.w <= 0.0 { return 1.0; }
    let p = projected.xyz / projected.w;
    let uv = vec2(p.x*0.5+0.5,0.5-p.y*0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) || p.z < 0.0 || p.z > 1.0 { return 1.0; }
    var result=0.0;
    let scale = select(shadows.filter_scale.x,shadows.filter_scale.y/projected.w,layer==1u);
    // Four dispersed bilinear taps. The fire's edge softens with distance,
    // while its contact stays tight. Radius is in metres, independent of zoom.
    let kernel=array<vec2<f32>,4>(vec2(-0.75,-0.25),vec2(0.25,-0.75),
        vec2(0.75,0.25),vec2(-0.25,0.75));
    for (var i=0u; i<4u; i+=1u) {
        result+=textureSampleCompareLevel(shadow_depth,shadow_sampler,
            uv+kernel[i]*radius*scale,i32(layer),p.z-0.000002);
    }
    return result/4.0;
}

fn point_visibility(index: u32, position: vec3<f32>, normal: vec3<f32>) -> f32 {
    if index != shadows.enabled.z { return 1.0; }
    return visibility(1u,position,normal);
}

// The orthographic view direction is fixed by the shared isometric contract.
// Wet highlights are presentation-only and vanish when a dry scene is selected.
fn wet_highlight(normal: vec3<f32>, direction: vec3<f32>) -> f32 {
    let sum = direction + vec3<f32>(0.57735027);
    let half_vector = sum * inverseSqrt(max(dot(sum,sum),0.0001));
    return pow(max(dot(normalize(normal), half_vector), 0.0), 48.0)
        * max(dot(normalize(normal), direction), 0.0);
}

fn surface_colour(base: vec3<f32>, world_pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let unit_normal = normalize(normal);
    let wet = scene_lights.direction.w != 0.0;
    let directional_shadow = visibility(0u, world_pos, unit_normal);
    let facing = max(dot(unit_normal, scene_lights.direction.xyz), 0.0);
    // Sky fill favors upward surfaces; undersides stay dark enough to read form.
    let sky = 0.35 + 0.65 * max(unit_normal.y, 0.0);
    var total = scene_lights.ambient.rgb * scene_lights.ambient.w * sky
        + scene_lights.directional.rgb * scene_lights.directional.w * facing * directional_shadow;
    var sheen = vec3<f32>(0.0);
    if wet {
        sheen = scene_lights.directional.rgb * scene_lights.directional.w
            * wet_highlight(normal, scene_lights.direction.xyz) * directional_shadow;
    }
    for (var i=0u; i<scene_lights.count.x; i+=1u) {
        let light = scene_lights.lights[i];
        let offset = light.position_range.xyz-world_pos;
        let d2 = dot(offset,offset);
        // Softened inverse-square falloff avoids a singularity at the source.
        // Squared range fade reaches black without a hard ring.
        let range = light.position_range.w;
        let fade = max(1.0-d2/max(range*range,0.0001),0.0);
        let direction = offset*inverseSqrt(max(d2,0.0001));
        let emission = light.colour_intensity.rgb*light.colour_intensity.w;
        let shadow = point_visibility(i,world_pos,unit_normal);
        total += emission * max(dot(unit_normal,direction),0.0) * fade * fade / (1.0+d2) * shadow;
        if wet {
            sheen += emission * fade * fade / (1.0+d2) * wet_highlight(normal,direction) * shadow;
        }
    }
    return base * total + sheen * 0.22;
}
