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
