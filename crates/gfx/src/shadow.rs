//! Two bounded shadow maps: scene directional light and the nearest campfire.
use crate::OrthoCamera;
use arpg_core::{EnvironmentLight, PointLight};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt as _;

const SIZE: u32 = 2048;
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    matrices: [[f32; 16]; 2],
    enabled: [u32; 4],
    filter_scale: [f32; 4],
}

fn directional_view(focus: Vec3, direction: Vec3, span: f32) -> Mat4 {
    use glam::camera::rh::{proj::directx, view::look_at_mat4};
    let up = if direction.y.abs() > 0.98 { Vec3::Z } else { Vec3::Y };
    let view = look_at_mat4(direction * 80.0, Vec3::ZERO, up);
    let mut centre = view.transform_point3(focus);
    let texel = 2.0 * span / SIZE as f32;
    centre.x = (centre.x / texel).round() * texel;
    centre.y = (centre.y / texel).round() * texel;
    let snapped = view.inverse().transform_point3(centre);
    // Lock the light's XY grid to the world. Camera easing must not make
    // stationary silhouettes crawl across shadow texels.
    directx::orthographic(-span, span, -span, span, 0.1, 160.0)
        * look_at_mat4(snapped + direction * 80.0, snapped, up)
}

pub(crate) struct ShadowMaps {
    pub(crate) view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) uniform: wgpu::Buffer,
    layers: [wgpu::TextureView; 2],
    cameras: [wgpu::Buffer; 2],
    bindings: [wgpu::BindGroup; 2],
    enabled: [bool; 2],
    matrices: [Mat4; 2],
}

fn camera_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow camera layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(64),
            },
            count: None,
        }],
    })
}

impl ShadowMaps {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("directional and fire shadow depths"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layers = std::array::from_fn(|layer| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow comparison"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shadow transforms and selection"),
            contents: bytemuck::bytes_of(&Uniform {
                matrices: [Mat4::IDENTITY.to_cols_array(); 2],
                enabled: [0; 4],
                filter_scale: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = camera_layout(device);
        let cameras = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shadow view"),
                size: 64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let bindings = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shadow view"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cameras[i].as_entire_binding(),
                }],
            })
        });
        Self {
            view,
            sampler,
            uniform,
            layers,
            cameras,
            bindings,
            enabled: [false; 2],
            matrices: [Mat4::IDENTITY; 2],
        }
    }

    pub(crate) fn update(
        &mut self,
        queue: &wgpu::Queue,
        camera: &OrthoCamera,
        environment: EnvironmentLight,
        lights: &[PointLight],
    ) {
        use glam::camera::rh::{proj::directx, view::look_at_mat4};
        let focus = camera.target();
        let direction = environment.direction().normalize();
        let span = camera.view_height().clamp(16.0, 64.0);
        let directional = directional_view(focus, direction, span);
        let primary = lights
            .iter()
            .enumerate()
            .filter(|(_, light)| light.intensity() > 0.0 && light.position().y > 0.05)
            .min_by(|(_, a), (_, b)| {
                a.position()
                    .distance_squared(focus)
                    .total_cmp(&b.position().distance_squared(focus))
            });
        let fire = primary.map_or(Mat4::IDENTITY, |(_, light)| {
            let position = light.position();
            directx::perspective(
                160.0_f32.to_radians(),
                1.0,
                0.08,
                (light.range() * 2.0).clamp(8.0, 160.0),
            ) * look_at_mat4(position, position - Vec3::Y, Vec3::Z)
        });
        self.matrices=[directional,fire];
        self.enabled = [environment.directional_intensity() > 0.0, primary.is_some()];
        let packed = Uniform {
            matrices: [directional.to_cols_array(), fire.to_cols_array()],
            enabled: [
                u32::from(self.enabled[0]),
                u32::from(self.enabled[1]),
                primary.map_or(0, |(i, _)| i as u32),
                0,
            ],
            // UV per metre; perspective scale still divides by projected W.
            filter_scale: [0.5/span, 0.5/80.0_f32.to_radians().tan(), 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&packed));
        for (buffer, matrix) in self.cameras.iter().zip(packed.matrices) {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&matrix));
        }
    }

    pub(crate) fn matrices(&self) -> impl Iterator<Item=Mat4> {
        self.matrices.into_iter().zip(self.enabled).filter_map(|(matrix,enabled)| enabled.then_some(matrix))
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = (&wgpu::TextureView, &wgpu::BindGroup)> {
        self.layers
            .iter()
            .zip(&self.bindings)
            .zip(self.enabled)
            .filter_map(|((view, binding), enabled)| enabled.then_some((view, binding)))
    }
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    materials_and_joints: &[&wgpu::BindGroupLayout],
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
) -> wgpu::RenderPipeline {
    let camera = camera_layout(device);
    let layouts: Vec<_> = std::iter::once(Some(&camera))
        .chain(materials_and_joints.iter().map(|layout| Some(*layout)))
        .collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shadow depth layout"),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("animated caster depth"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers,
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState {
                constant: 1,
                slope_scale: 1.0,
                clamp: 0.0,
            },
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MeshBatch, camera::CameraBinding, capture::Readback, material::MaterialLayout,
        mesh::MeshPipeline,
    };
    use arpg_core::{Instance, LinearRgb};

    #[test]
    fn directional_grid_stays_fixed_for_camera_motion_smaller_than_a_texel() {
        let direction = Vec3::new(-0.6, 1.0, -0.3).normalize();
        let right = direction.cross(Vec3::Y).normalize();
        let span = 24.0;
        let texel = 2.0 * span / SIZE as f32;
        let projected = |focus| directional_view(focus, direction, span)
            .project_point3(Vec3::new(1.0, 0.0, 2.0)).truncate();
        let origin = projected(Vec3::ZERO);
        assert!(origin.abs_diff_eq(projected(right * texel * 0.2), 1e-6));
        assert!(origin.abs_diff_eq(projected(-right * texel * 0.2), 1e-6));
        let next = projected(right * texel);
        assert!((origin.distance(next) - 2.0/SIZE as f32).abs() < 1e-6);
    }

    #[test]
    fn coarse_horde_caster_moves_fire_and_directional_shadows_with_the_visible_pose() {
        use crate::{CharacterBucket, CharacterHorde, MAX_HORDE_POSE_BUCKETS, character::CharacterPipeline};
        const N: u32 = 256;
        let (device, queue) = crate::tests::headless_device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut binding = CameraBinding::new(&device);
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let floor_pipeline = MeshPipeline::new(&device, &binding.layout, MaterialLayout::new(&device), format, crate::DEPTH_FORMAT);
        let ground = arpg_assets::import_glb(include_bytes!("../../../assets/world/ground.glb")).unwrap();
        let floor = floor_pipeline.upload_mesh(&device, &queue, &ground).unwrap();
        let mut asset = arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/wolf/wolf.glb")).unwrap();
        asset.attach_shadow(arpg_assets::import_character_glb(include_bytes!("../../../assets/characters/wolf/wolf.shadow.glb")).unwrap()).unwrap();
        let mut characters = CharacterPipeline::new(&device, &binding.layout, MaterialLayout::new(&device), format, crate::DEPTH_FORMAT);
        let mesh = characters.upload_character(&device, &queue, &asset).unwrap();
        let mut camera = OrthoCamera::new(N, N);
        camera.zoom_by(0.3);
        binding.upload(&queue, &camera);
        let colour = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("animated coarse caster evidence"),
            size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = colour.create_view(&Default::default());
        let depth = crate::create_depth(&device, N, N);
        let mut render = |present: bool, seconds: f32, lights: &[PointLight], environment: EnvironmentLight| {
            let floor_instances = [Instance::new(Vec3::ZERO, Vec3::new(12.0, 1.0, 12.0), LinearRgb::WHITE)];
            let batches = [MeshBatch::new(&floor, &floor_instances)];
            floor_pipeline.upload(&queue, &batches);
            binding.upload_lights(&queue, lights, environment, false);
            binding.shadows.update(&queue, &camera, environment, lights);
            let mut pose = asset.bind_pose();
            asset.sample(asset.clip_named("Run").unwrap(), seconds, &mut pose);
            let instances = [Instance::new(Vec3::ZERO, Vec3::ONE, LinearRgb::WHITE)];
            let empty = CharacterBucket::new(&pose, &[]);
            let mut buckets = [empty; MAX_HORDE_POSE_BUCKETS];
            if present { buckets[2] = CharacterBucket::new(&pose, &instances); }
            let horde = CharacterHorde::new(&mesh, buckets);
            let draws = characters.upload(&queue, None, Some(horde), &[crate::visibility::Frustum::new(camera.view_proj())]);
            let mut encoder = device.create_command_encoder(&Default::default());
            for (view, camera) in binding.shadows.views() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None, color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None,
                    }), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                });
                floor_pipeline.draw_depth(&mut pass, camera, &batches);
                characters.draw_depth(&mut pass, camera, None, Some(horde), &draws);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view, depth_slice: None, resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None,
                    }), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                });
                // Only the receiver is visible: changed pixels must be cast shadows.
                floor_pipeline.draw_batches(&mut pass, &binding, &batches);
            }
            let readback = Readback::new(&device, N, N);
            readback.record(&mut encoder, &colour);
            queue.submit(Some(encoder.finish()));
            readback.to_rgba(&device).unwrap()
        };
        let dark = EnvironmentLight::try_new(LinearRgb::WHITE, 0.0, LinearRgb::WHITE, 0.0, Vec3::Y).unwrap();
        let moon = EnvironmentLight::try_new(LinearRgb::WHITE, 0.0, LinearRgb::WHITE, 0.7, Vec3::new(-0.6, 1.0, -0.3)).unwrap();
        let fire = [PointLight::new(Vec3::new(-2.0, 3.0, -1.0), LinearRgb::WHITE, 35.0, 16.0)];
        let difference = |a: &[u8], b: &[u8]| a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).filter(|(a,b)| a[..3] != b[..3]).count();
        for (lights, environment) in [(&fire[..], dark), (&[][..], moon)] {
            let clean = render(false, 0.0, lights, environment);
            let start = render(true, 0.0, lights, environment);
            let moved = render(true, 0.25, lights, environment);
            assert!(difference(&clean, &start) > 20, "coarse caster must interrupt light");
            assert!(difference(&start, &moved) > 10, "shadow must follow the sampled moving limbs");
            assert_eq!(start, render(true, 0.0, lights, environment));
            assert_eq!(clean, render(false, 0.25, lights, environment), "removed instance leaves no stale shadow");
        }
        assert!(pollster::block_on(scope.pop()).is_none());
    }

    #[test]
    fn moving_geometry_casts_and_releases_fire_and_directional_shadows() {
        const N: u32 = 256;
        let (device, queue) = crate::tests::headless_device();
        let mut binding = CameraBinding::new(&device);
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let pipeline = MeshPipeline::new(
            &device,
            &binding.layout,
            MaterialLayout::new(&device),
            format,
            crate::DEPTH_FORMAT,
        );
        let ground =
            arpg_assets::import_glb(include_bytes!("../../../assets/world/ground.glb")).unwrap();
        let prop =
            arpg_assets::import_glb(include_bytes!("../../../assets/world/prop.glb")).unwrap();
        let floor = pipeline.upload_mesh(&device, &queue, &ground).unwrap();
        let caster = pipeline.upload_mesh(&device, &queue, &prop).unwrap();
        let camera = OrthoCamera::new(N, N);
        binding.upload(&queue, &camera);
        let colour = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow evidence"),
            size: wgpu::Extent3d {
                width: N,
                height: N,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = colour.create_view(&Default::default());
        let depth = crate::create_depth(&device, N, N);
        let mut render = |x: f32, lights: &[PointLight], environment: EnvironmentLight| {
            let floor_instances = [Instance::new(
                Vec3::ZERO,
                Vec3::new(24.0, 1.0, 24.0),
                LinearRgb::WHITE,
            )];
            let caster_instances = [Instance::new(
                Vec3::X * x,
                Vec3::new(2.0, 4.0, 2.0),
                LinearRgb::WHITE,
            )];
            let batches = [
                MeshBatch::new(&floor, &floor_instances),
                MeshBatch::new(&caster, &caster_instances),
            ];
            pipeline.upload(&queue, &batches);
            binding.upload_lights(&queue, lights, environment, false);
            binding.shadows.update(&queue, &camera, environment, lights);
            let mut encoder = device.create_command_encoder(&Default::default());
            for (view, camera) in binding.shadows.views() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pipeline.draw_depth(&mut pass, camera, &batches);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pipeline.draw_batches(&mut pass, &binding, &batches);
            }
            let readback = Readback::new(&device, N, N);
            readback.record(&mut encoder, &colour);
            queue.submit(Some(encoder.finish()));
            readback.to_rgba(&device).unwrap()
        };
        let pixel = |rgba: &[u8]| {
            let p = camera.view_proj() * Vec3::new(1.15, 0.0, 0.0).extend(1.0);
            let x = ((p.x * 0.5 + 0.5) * N as f32) as usize;
            let y = ((0.5 - p.y * 0.5) * N as f32) as usize;
            rgba[(y * N as usize + x) * 4]
        };
        let darkness =
            EnvironmentLight::try_new(LinearRgb::WHITE, 0.0, LinearRgb::WHITE, 0.0, Vec3::Y)
                .unwrap();
        let daylight = EnvironmentLight::try_new(
            LinearRgb::WHITE,
            0.0,
            LinearRgb::WHITE,
            0.7,
            Vec3::new(-1.0, 1.0, 0.0),
        )
        .unwrap();
        let fire = [PointLight::new(
            Vec3::new(-3.0, 5.0, 0.0),
            LinearRgb::WHITE,
            35.0,
            16.0,
        )];
        for (lights, environment) in [(&fire[..], darkness), (&[][..], daylight)] {
            let shadow = pixel(&render(0.0, lights, environment));
            let revealed = pixel(&render(4.0, lights, environment));
            assert!(
                revealed > 40 && shadow < revealed / 2,
                "caster must interrupt light: {shadow} -> {revealed}"
            );
            assert_eq!(
                pixel(&render(0.0, lights, environment)),
                shadow,
                "returning geometry restores its shadow"
            );
        }
    }
}
