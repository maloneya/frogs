use arpg_core::{Instance, LinearRgb};
use super::*;
use crate::{
    MeshBatch, OrthoCamera, camera::CameraBinding, capture::Readback, material::MaterialLayout,
    mesh::MeshPipeline,
};
use glam::Vec3;

/// Exercise the actual fragment shader on a four-vertex floor: interpolating
/// illumination at its corners would miss the entire pool of light.
#[test]
fn environment_and_torches_contribute_independently_in_linear_colour() {
    const N: u32 = 256;
    let (device, queue) = crate::tests::headless_device();
    let binding = CameraBinding::new(&device);
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let pipeline = MeshPipeline::new(
        &device,
        &binding.layout,
        MaterialLayout::new(&device),
        format,
        crate::DEPTH_FORMAT,
    );
    let mesh =
        arpg_assets::import_glb(include_bytes!("../../../../assets/world/ground.glb")).unwrap();
    let gpu = pipeline.upload_mesh(&device, &queue, &mesh).unwrap();
    let instances = [Instance::new(
        Vec3::ZERO,
        Vec3::new(24.0, 1.0, 24.0),
        LinearRgb::WHITE,
    )];
    let batches = [MeshBatch::new(&gpu, &instances)];
    pipeline.upload(&queue, &batches);
    let camera = OrthoCamera::new(N, N);
    binding.upload(&queue, &camera);
    let colour = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("point light test"),
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
    let render_surface = |lights: &[PointLight], environment: EnvironmentLight, wet: bool| {
        binding.upload_lights(&queue, lights, environment, wet);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point light test"),
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
    let render = |lights: &[PointLight], environment: EnvironmentLight| render_surface(lights, environment, false);
    let sample = |rgba: &[u8], position: Vec3| {
        let clip = camera.view_proj() * position.extend(1.0);
        let x = ((clip.x / clip.w * 0.5 + 0.5) * N as f32) as usize;
        let y = ((0.5 - clip.y / clip.w * 0.5) * N as f32) as usize;
        let offset = (y * N as usize + x) * 4;
        [rgba[offset], rgba[offset + 1], rgba[offset + 2]]
    };
    let daylight = EnvironmentLight::default();
    let darkness = EnvironmentLight::try_new(LinearRgb::WHITE, 0.0, LinearRgb::WHITE, 0.0, Vec3::Y).unwrap();
    let neutral_frame = render(&[], daylight);
    // Predict: warm bright centre, much dimmer at four metres, black beyond
    // eight. A light below the upward-facing floor illuminates none of it.
    let warm = LinearRgb::new(1.0, 0.5, 0.1);
    let frame = render(&[PointLight::new(Vec3::Y * 2.0, warm, 12.0, 8.0)], darkness);
    let near = sample(&frame, Vec3::ZERO);
    let far = sample(&frame, Vec3::X * 4.0);
    assert!(
        near[0] > 50 && near[0] > near[1] && near[1] > near[2],
        "warm centre: {near:?}"
    );
    assert!(
        near[0] > far[0].saturating_mul(2) && far[0] > 0,
        "distance falloff: {near:?} to {far:?}"
    );
    assert_eq!(sample(&frame, Vec3::X * 9.0), [0; 3]);
    for light in [
        PointLight::new(-Vec3::Y * 2.0, warm, 12.0, 8.0),
        PointLight::new(Vec3::Y * 2.0, warm, 0.0, 8.0),
    ] {
        let dark = render(&[light], darkness);
        assert!(
            dark.as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3] == [0; 3]),
            "no back-face or ambient illumination"
        );
    }

    // Contributions add in linear colour, and each channel retains its source.
    let red = PointLight::new(Vec3::Y * 2.0, LinearRgb::new(1.0, 0.0, 0.0), 6.0, 8.0);
    let blue = PointLight::new(Vec3::Y * 2.0, LinearRgb::new(0.0, 0.0, 1.0), 6.0, 8.0);
    let red_pixel = sample(&render(&[red], darkness), Vec3::ZERO);
    let blue_pixel = sample(&render(&[blue], darkness), Vec3::ZERO);
    let mixed = sample(&render(&[red, blue], darkness), Vec3::ZERO);
    assert_eq!(
        mixed,
        [red_pixel[0], 0, blue_pixel[2]],
        "red and blue must both reach the same pixel"
    );
    assert_eq!(sample(&render(&[blue, red], darkness), Vec3::ZERO), mixed);
    let linear = |byte: u8| {
        let srgb = f32::from(byte) / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    for count in [2, MAX_POINT_LIGHTS] {
        let accumulated = sample(&render(&vec![red; count], darkness), Vec3::ZERO);
        assert!(accumulated[0] < 255, "fixture must not clip");
        assert!(
            (linear(accumulated[0]) - linear(red_pixel[0]) * count as f32).abs() < 0.006,
            "{count} lights must add before sRGB encoding: {red_pixel:?} to {accumulated:?}"
        );
    }
    // Explicit darkness remains black with no torches; their count never
    // selects an environment. A zero-intensity torch cannot extinguish daylight.
    assert!(render(&[], darkness).as_chunks::<4>().0.iter().all(|pixel| pixel[..3] == [0; 3]));
    let zero = PointLight::new(Vec3::Y * 2.0, warm, 0.0, 8.0);
    assert_eq!(render(&[zero], daylight), neutral_frame);
    assert_eq!(render(&[], daylight), neutral_frame, "clearing slots preserves authored daylight");
    let centre = sample(&neutral_frame, Vec3::ZERO);
    assert!(centre[0] > 50 && centre[0] < 230, "readable floor: {centre:?}");
    assert_eq!(sample(&neutral_frame, Vec3::X * 9.0), centre);

    // Sky fill is independent of directional-light facing. Directional colour lands
    // only on surfaces facing the light, and adds before sRGB output encoding.
    let ambient = EnvironmentLight::try_new(LinearRgb::new(1.0, 0.0, 0.0), 0.12, LinearRgb::new(0.0, 0.0, 1.0), 0.25, Vec3::Y).unwrap();
    let opposite = EnvironmentLight::try_new(LinearRgb::new(1.0, 0.0, 0.0), 0.12, LinearRgb::new(0.0, 0.0, 1.0), 0.25, -Vec3::Y).unwrap();
    let both = sample(&render(&[], ambient), Vec3::ZERO);
    let fill = sample(&render(&[], opposite), Vec3::ZERO);
    assert!(both[0] > 0 && both[1] == 0 && both[2] > 0, "coloured fill and directional: {both:?}");
    assert_eq!(fill, [both[0], 0, 0], "back-facing directional cannot remove ambient fill");
    let combined = sample(&render(&[red], opposite), Vec3::ZERO);
    assert!((linear(combined[0]) - linear(fill[0]) - linear(red_pixel[0])).abs() < 0.006,
        "point and ambient illumination must add in linear colour");
    // Fire opposite the viewing direction reflects toward the camera on this
    // floor. Wetness adds that warm highlight and clearing it restores dry pixels.
    let reflected = [PointLight::new(Vec3::new(-2.0, 2.0, -2.0), warm, 12.0, 8.0)];
    let dry = render(&reflected, darkness);
    let wet = render_surface(&reflected, darkness, true);
    let dry_pixel = sample(&dry, Vec3::ZERO);
    let wet_pixel = sample(&wet, Vec3::ZERO);
    assert!(wet_pixel[0] > dry_pixel[0].saturating_add(5), "wet highlight: {dry_pixel:?} to {wet_pixel:?}");
    assert!(wet_pixel[0] > wet_pixel[1] && wet_pixel[1] > wet_pixel[2], "wet highlight retains the fire's colour");
    assert_eq!(render(&reflected, darkness), dry, "clearing weather restores dry shading");
    assert_eq!(render_surface(&[], darkness, true), render(&[], darkness), "wetness cannot emit light");
}

#[test]
fn gpu_admission_rejects_a_ninth_light() {
    let light = PointLight::new(Vec3::Y, LinearRgb::WHITE, 1.0, 2.0);
    assert!(
        std::panic::catch_unwind(|| LightUniform::new(&[light; MAX_POINT_LIGHTS + 1], EnvironmentLight::default())).is_err()
    );
}

/// Freeze the pre-optimization contribution equation as the GPU oracle. Both
/// variants use the current shadow lookup; this isolates contribution admission.
#[test]
fn zero_contribution_admission_preserves_rendered_lighting() {
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 8;
    const PROBE: &str = r#"
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let triangle = array<vec2<f32>,3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    return vec4(triangle[i],0.0,1.0);
}
@fragment fn fs_main(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let normals = array<vec3<f32>,8>(vec3(0.0,1.0,0.0),vec3(0.0,-1.0,0.0),
        vec3(1.0,0.0,0.0),vec3(-1.0,0.0,0.0),vec3(0.0,0.0,1.0),vec3(0.0,0.0,-1.0),
        normalize(vec3(1.0,2.0,3.0)),normalize(vec3(-2.0,1.0,-3.0)));
    let x = u32(pixel.x);
    let position = vec3(f32(x%16u)-8.0,f32(x/16u),0.0);
    return vec4(surface_colour(vec3(0.37,0.21,0.14),position,normals[u32(pixel.y)]),1.0);
}
"#;
    const REFERENCE_CONTRIBUTIONS: &str = r#"
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
"#;
    let (device, queue) = crate::tests::headless_device();
    let mut binding = CameraBinding::new(&device);
    let camera = OrthoCamera::new(WIDTH, HEIGHT);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lighting admission equivalence"),
        bind_group_layouts: &[Some(&binding.layout)],
        immediate_size: 0,
    });
    let current = include_str!("../lighting.wgsl");
    let reference = format!(
        "{}{}",
        current.split_once("fn surface_colour(").unwrap().0,
        REFERENCE_CONTRIBUTIONS
    );
    let make_pipeline =
        |contributions: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lighting admission oracle"),
            source: wgpu::ShaderSource::Wgsl(format!(
                "const MAX_POINT_LIGHTS: u32 = {MAX_POINT_LIGHTS}u;\n{contributions}\n{PROBE}"
            ).into()),
        });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("lighting admission oracle"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::TextureFormat::Bgra8UnormSrgb.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
    let reference_pipeline = make_pipeline(&reference);
    let current_pipeline = make_pipeline(current);
    let colour = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lighting admission pixels"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = colour.create_view(&Default::default());
    let render = |pipeline: &wgpu::RenderPipeline, binding: &CameraBinding| {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lighting admission pixels"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &binding.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let readback = Readback::new(&device, WIDTH, HEIGHT);
        readback.record(&mut encoder, &colour);
        queue.submit(Some(encoder.finish()));
        readback.to_rgba(&device).unwrap()
    };
    // Probe the shared fragment function directly: static and character shaders
    // supply the same world position/normal inputs. Rows cover front, back,
    // tangent and oblique normals; columns include points inside, exactly on,
    // and outside the two-metre light range at floor and character heights.
    let warm = PointLight::new(Vec3::Y * 2.0, LinearRgb::new(1.0, 0.5, 0.1), 4.0, 2.0);
    let cool = PointLight::new(
        Vec3::new(3.0, 2.0, 0.0),
        LinearRgb::new(0.1, 0.3, 1.0),
        2.0,
        4.0,
    );
    let below = PointLight::new(-Vec3::Y * 2.0, LinearRgb::WHITE, 2.0, 4.0);
    let zero = warm.with_scaled_intensity(0.0);
    let tiny = PointLight::new(Vec3::Y, LinearRgb::WHITE, 0.1, 0.01);
    let light_sets: &[&[PointLight]] = &[&[], &[warm], &[warm, cool, below, zero, tiny]];
    let mut lit = None;
    for wet in [false, true] {
        for direction in [Vec3::Y, -Vec3::Y] {
            let environment = EnvironmentLight::try_new(
                LinearRgb::WHITE,
                0.12,
                LinearRgb::WHITE,
                0.35,
                direction,
            )
            .unwrap();
            for &lights in light_sets {
                binding.upload_lights(&queue, lights, environment, wet);
                binding.shadows.update(&queue, &camera, environment, lights);
                for shadow_depth in [0.0, 1.0] {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    for (_, view, _) in binding.shadows.views() {
                        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("controlled shadow oracle"),
                            color_attachments: &[],
                            depth_stencil_attachment: Some(
                                wgpu::RenderPassDepthStencilAttachment {
                                    view,
                                    depth_ops: Some(wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(shadow_depth),
                                        store: wgpu::StoreOp::Store,
                                    }),
                                    stencil_ops: None,
                                },
                            ),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                    }
                    queue.submit(Some(encoder.finish()));
                    let expected = render(&reference_pipeline, &binding);
                    assert_eq!(
                        render(&current_pipeline, &binding),
                        expected,
                        "wet={wet}, direction={direction:?}, lights={}, shadow={shadow_depth}",
                        lights.len()
                    );
                    if !wet && direction == Vec3::Y && lights.len() == 1 {
                        if shadow_depth == 0.0 {
                            lit = Some(expected);
                        } else {
                            assert_ne!(
                                lit.as_ref().unwrap(),
                                &expected,
                                "oracle must exercise shadow comparisons"
                            );
                        }
                    }
                }
            }
        }
    }
}
