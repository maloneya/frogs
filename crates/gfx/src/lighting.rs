//! Packing bounded light lists for both surface shaders.
use arpg_core::{MAX_POINT_LIGHTS, MAX_DIRECTIONAL_LIGHTS, DirectionalLight, PointLight};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightUniform {
    count: [u32; 4],
    lights: [[[f32; 4]; 2]; MAX_POINT_LIGHTS],
    directional: [[[f32; 4]; 2]; MAX_DIRECTIONAL_LIGHTS],
}

impl LightUniform {
    pub(crate) fn new(lights: &[PointLight], directional: &[DirectionalLight]) -> Self {
        assert!(lights.len() <= MAX_POINT_LIGHTS, "too many point lights");
        assert!(directional.len() <= MAX_DIRECTIONAL_LIGHTS, "too many directional lights");
        let mut uniform = Self {
            count: [lights.len() as u32, directional.len() as u32, 0, 0],
            lights: [[[0.0; 4]; 2]; MAX_POINT_LIGHTS],
            directional: [[[0.0; 4]; 2]; MAX_DIRECTIONAL_LIGHTS],
        };
        for (target, light) in uniform.lights.iter_mut().zip(lights) {
            *target = [
                light.position().extend(light.range()).to_array(),
                light.colour().extend(light.intensity()).to_array(),
            ];
        }
        for (target, light) in uniform.directional.iter_mut().zip(directional) {
            *target = [light.toward_source().extend(light.fill()).to_array(), light.colour().extend(light.intensity()).to_array()];
        }
        uniform
    }
}

pub(crate) fn shader(device: &wgpu::Device, surface: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lit surface"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "const MAX_POINT_LIGHTS: u32 = {MAX_POINT_LIGHTS}u;\nconst MAX_DIRECTIONAL_LIGHTS: u32 = {MAX_DIRECTIONAL_LIGHTS}u;\n{}\n{surface}",
                include_str!("lighting.wgsl")
            )
            .into(),
        ),
    })
}

#[cfg(test)]
#[path = "lighting/tests.rs"]
mod gpu_tests;
