//! Shared GPU representation of the base-colour material both asset paths use.

use wgpu::util::DeviceExt as _;

#[derive(Clone)]
pub(crate) struct MaterialLayout(wgpu::BindGroupLayout);

impl MaterialLayout {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self(
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("asset base colour material"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            }),
        )
    }

    pub(crate) fn raw(&self) -> &wgpu::BindGroupLayout {
        &self.0
    }
}

pub(crate) struct Material {
    bind_group: wgpu::BindGroup,
}

impl Material {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &MaterialLayout,
        image: &arpg_assets::BaseColorTexture,
        base_color_factor: [f32; 4],
        label: &'static str,
    ) -> Self {
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: image.width(),
                    height: image.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: image.mip_level_count(),
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Sampling this format decodes authored sRGB texels before the
                // shader multiplies them by linear lighting and factors.
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            image.rgba8_mip_chain(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let factor = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(&[base_color_factor]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Self {
            bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: layout.raw(),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: factor.as_entire_binding(),
                    },
                ],
            }),
        }
    }

    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}
