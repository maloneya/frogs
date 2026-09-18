use super::*;
use crate::{
    MeshBatch, OrthoCamera, camera::CameraBinding, capture::Readback, material::MaterialLayout,
    mesh::MeshPipeline,
};
use arpg_core::Instance;
use glam::Vec3;

/// Exercise the actual fragment shader on a four-vertex floor: interpolating
/// illumination at its corners would miss the entire pool of light.
#[test]
fn floor_has_a_warm_pool_distance_falloff_and_no_ambient() {
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
        Vec3::ONE,
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
    let render = |lights: &[PointLight]| {
        binding.upload_lights(&queue, lights);
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
    let sample = |rgba: &[u8], position: Vec3| {
        let clip = camera.view_proj() * position.extend(1.0);
        let x = ((clip.x / clip.w * 0.5 + 0.5) * N as f32) as usize;
        let y = ((0.5 - clip.y / clip.w * 0.5) * N as f32) as usize;
        let offset = (y * N as usize + x) * 4;
        [rgba[offset], rgba[offset + 1], rgba[offset + 2]]
    };
    // Predict: warm bright centre, much dimmer at four metres, black beyond
    // eight. A light below the upward-facing floor illuminates none of it.
    let warm = Vec3::new(1.0, 0.5, 0.1);
    let frame = render(&[PointLight::new(Vec3::Y * 2.0, warm, 12.0, 8.0)]);
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
        let dark = render(&[light]);
        assert!(
            dark.as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3] == [0; 3]),
            "no back-face or ambient illumination"
        );
    }

    // Contributions add in linear colour, and each channel retains its source.
    let red = PointLight::new(Vec3::Y * 2.0, Vec3::X, 6.0, 8.0);
    let blue = PointLight::new(Vec3::Y * 2.0, Vec3::Z, 6.0, 8.0);
    let red_pixel = sample(&render(&[red]), Vec3::ZERO);
    let blue_pixel = sample(&render(&[blue]), Vec3::ZERO);
    let mixed = sample(&render(&[red, blue]), Vec3::ZERO);
    assert_eq!(
        mixed,
        [red_pixel[0], 0, blue_pixel[2]],
        "red and blue must both reach the same pixel"
    );
    assert_eq!(sample(&render(&[blue, red]), Vec3::ZERO), mixed);
    let linear = |byte: u8| {
        let srgb = f32::from(byte) / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    for count in [2, MAX_POINT_LIGHTS] {
        let accumulated = sample(&render(&vec![red; count]), Vec3::ZERO);
        assert!(accumulated[0] < 255, "fixture must not clip");
        assert!(
            (linear(accumulated[0]) - linear(red_pixel[0]) * count as f32).abs() < 0.006,
            "{count} lights must add before sRGB encoding: {red_pixel:?} to {accumulated:?}"
        );
    }
    // A shorter list must clear all previously uploaded slots, including the eighth.
    let dark = render(&[]);
    assert!(
        dark.as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [0; 3])
    );
}

#[test]
fn gpu_admission_rejects_a_ninth_light() {
    let light = PointLight::new(Vec3::Y, Vec3::ONE, 1.0, 2.0);
    assert!(
        std::panic::catch_unwind(|| LightUniform::new(&[light; MAX_POINT_LIGHTS + 1])).is_err()
    );
}
