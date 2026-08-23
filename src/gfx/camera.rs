use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec3};

/// A true isometric camera.
///
/// "Isometric" is a specific claim, not a vibe: an orthographic projection at
/// 45° yaw and an elevation of `atan(1/√2)` ≈ 35.264°. At exactly that angle a
/// unit cube projects to a regular hexagon and its three visible faces have
/// equal area — hence *iso*-metric, equal measure. Any other elevation is some
/// other axonometric projection.
pub struct OrthoCamera {
    pub target: Vec3,
    /// Half-height of the view volume, in world units. Smaller = zoomed in.
    pub zoom: f32,
    pub aspect: f32,
}

const ISO_YAW: f32 = std::f32::consts::FRAC_PI_4; // 45°
const ISO_PITCH: f32 = 0.615_479_7; // atan(1/sqrt(2))

/// How far back the camera sits. Under orthographic projection this has *no*
/// effect on apparent size — there's no perspective divide — so it only has to
/// be large enough that the near plane never clips the scene.
const DISTANCE: f32 = 250.0;

impl OrthoCamera {
    pub fn new(aspect: f32) -> Self {
        Self { target: Vec3::ZERO, zoom: 26.0, aspect }
    }

    pub fn view_proj(&self) -> Mat4 {
        let dir = Vec3::new(
            ISO_PITCH.cos() * ISO_YAW.cos(),
            ISO_PITCH.sin(),
            ISO_PITCH.cos() * ISO_YAW.sin(),
        );
        let eye = self.target + dir * DISTANCE;
        let view = look_at_mat4(eye, self.target, Vec3::Y);

        let half_h = self.zoom;
        let half_w = self.zoom * self.aspect;

        // glam names projections by the NDC convention they target, not just by
        // handedness. `directx` is the wgpu one: Y-up with Z in [0, 1]. The
        // `opengl` variant maps Z to [-1, 1] and would silently squash the whole
        // scene into the near half of the depth buffer; `vulkan` flips Y and
        // would render the world upside down.
        //
        // Orthographic depth is *linear*, unlike perspective, so a generous
        // near/far range costs nothing in precision.
        let proj = directx::orthographic(-half_w, half_w, -half_h, half_h, 0.1, DISTANCE * 2.0);

        proj * view
    }
}

/// The GPU-side half of the camera: one 64-byte uniform holding the combined
/// view-projection matrix, plus the bind group that makes it visible to shaders.
pub struct CameraBinding {
    buffer: wgpu::Buffer,
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

impl CameraBinding {
    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera uniform"),
            size: 64, // one mat4x4<f32>
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
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
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self { buffer, layout, bind_group }
    }

    /// One of only two places per frame where data crosses into the GPU.
    pub fn upload(&self, queue: &wgpu::Queue, camera: &OrthoCamera) {
        let m = camera.view_proj();
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&m.to_cols_array()));
    }
}
