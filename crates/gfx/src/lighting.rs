//! Packing bounded light lists for both surface shaders.
use arpg_core::{EnvironmentLight, MAX_POINT_LIGHTS, PointLight};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightUniform {
    count: [u32; 4],
    ambient: [f32; 4],
    directional: [f32; 4],
    direction: [f32; 4],
    lights: [[[f32; 4]; 2]; MAX_POINT_LIGHTS],
}

impl LightUniform {
    pub(crate) fn new(lights: &[PointLight], environment: EnvironmentLight) -> Self {
        assert!(lights.len() <= MAX_POINT_LIGHTS, "too many point lights");
        let mut uniform = Self {
            count: [lights.len() as u32, 0, 0, 0],
            ambient: environment.ambient_colour().to_vec3().extend(environment.ambient_intensity()).to_array(),
            directional: environment.directional_colour().to_vec3().extend(environment.directional_intensity()).to_array(),
            direction: environment.direction().normalize().extend(0.0).to_array(),
            lights: [[[0.0; 4]; 2]; MAX_POINT_LIGHTS],
        };
        for (target, light) in uniform.lights.iter_mut().zip(lights) {
            *target = [
                light.position().extend(light.range()).to_array(),
                light.colour().to_vec3().extend(light.intensity()).to_array(),
            ];
        }
        uniform
    }
}

pub(crate) fn shader(device: &wgpu::Device, surface: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lit surface"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "const MAX_POINT_LIGHTS: u32 = {MAX_POINT_LIGHTS}u;\n{}\n{surface}",
                include_str!("lighting.wgsl")
            )
            .into(),
        ),
    })
}

#[cfg(test)]
#[path = "lighting/tests.rs"]
mod gpu_tests;
