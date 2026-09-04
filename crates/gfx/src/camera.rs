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
    target: Vec3,
    /// Half-height of the view volume, in world units. Smaller = zoomed in.
    zoom: f32,
    aspect: f32,
}

/// Zoom bounds, kept beside the field they constrain rather than in the input
/// handler that happens to drive it today. `zoom` reaching zero collapses the
/// view volume and produces a degenerate projection matrix — a black screen
/// with no error anywhere.
const MIN_ZOOM: f32 = 2.0;
const MAX_ZOOM: f32 = 400.0;

/// A zero-sized window is real — minimising produces one — and dividing by it
/// yields a NaN that propagates silently through the whole matrix.
fn aspect_of(width: u32, height: u32) -> f32 {
    width.max(1) as f32 / height.max(1) as f32
}

const ISO_YAW: f32 = std::f32::consts::FRAC_PI_4; // 45°
const ISO_PITCH: f32 = 0.615_479_7; // atan(1/sqrt(2))

/// How far back the camera sits. Under orthographic projection this has *no*
/// effect on apparent size — there's no perspective divide — so it only has to
/// be large enough that the near plane never clips the scene.
const DISTANCE: f32 = 250.0;

impl OrthoCamera {
    /// A camera framing the origin, sized to the given viewport in pixels.
    pub fn new(width: u32, height: u32) -> Self {
        Self { target: Vec3::ZERO, zoom: 26.0, aspect: aspect_of(width, height) }
    }

    /// Re-derives the aspect ratio after a resize.
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.aspect = aspect_of(width, height);
    }

    /// Scales the view volume. `factor < 1` zooms in, `> 1` zooms out; the
    /// clamp lives here so no caller has to remember the limits.
    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Where the camera sits, derived from the iso angles rather than stored,
    /// so the two can never disagree.
    fn eye(&self) -> Vec3 {
        let dir = Vec3::new(
            ISO_PITCH.cos() * ISO_YAW.cos(),
            ISO_PITCH.sin(),
            ISO_PITCH.cos() * ISO_YAW.sin(),
        );
        self.target + dir * DISTANCE
    }

    /// The screen's right and up directions, in world space, flattened onto the
    /// ground plane. Both unit length and perpendicular.
    ///
    /// Movement input arrives in *screen* space, because that is where the
    /// player experiences it: "up" means up on the monitor. Under a 45° yaw
    /// that is not any world axis — it is the diagonal `(-X, -Z)` — so treating
    /// "up" as world `+Z` sends the character off at 45° to where the player
    /// aimed. Every isometric game has to answer this, and answering it wrong
    /// is why some of them feel like driving on ice.
    ///
    /// Deriving the basis from the camera's own orientation rather than writing
    /// the diagonal down as a constant is what makes it stay correct the day
    /// the camera learns to rotate: the mapping moves with the view, for free,
    /// and nothing downstream has to be told.
    pub fn ground_basis(&self) -> (Vec3, Vec3) {
        let forward = (self.target - self.eye()).normalize();

        // `forward × Y` already lies in the XZ plane, so "right" needs no
        // flattening. Crossing back the other way recovers the ground-plane
        // component of forward — i.e. straight away from the camera, which is
        // what "up the screen" means on the floor.
        let right = forward.cross(Vec3::Y).normalize();
        (right, Vec3::Y.cross(right))
    }

    /// The combined view-projection matrix, as uploaded to the shader.
    pub fn view_proj(&self) -> Mat4 {
        let view = look_at_mat4(self.eye(), self.target, Vec3::Y);

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
pub(crate) struct CameraBinding {
    buffer: wgpu::Buffer,
    pub(crate) layout: wgpu::BindGroupLayout,
    pub(crate) bind_group: wgpu::BindGroup,
}

impl CameraBinding {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
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
    pub(crate) fn upload(&self, queue: &wgpu::Queue, camera: &OrthoCamera) {
        let m = camera.view_proj();
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&m.to_cols_array()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Projects a world point to normalised device coordinates. Under an
    /// orthographic projection `w` is 1, but dividing anyway keeps this honest
    /// if the camera ever gains perspective.
    fn to_ndc(camera: &OrthoCamera, world: Vec3) -> glam::Vec2 {
        let clip = camera.view_proj() * world.extend(1.0);
        clip.truncate().truncate() / clip.w
    }

    #[test]
    fn the_ground_basis_is_orthonormal_and_horizontal() {
        let (right, up) = OrthoCamera::new(1280, 720).ground_basis();

        for v in [right, up] {
            assert!((v.length() - 1.0).abs() < 1e-5, "not unit: {v}");
            assert!(v.y.abs() < 1e-6, "not on the ground plane: {v}");
        }
        assert!(right.dot(up).abs() < 1e-6, "not perpendicular");
    }

    /// The claim the basis actually makes, checked against the matrix that has
    /// to agree with it: walking along `up` moves *up the screen* and nowhere
    /// else. A sign error in either half passes the orthonormality test above
    /// and fails this one.
    #[test]
    fn walking_along_up_moves_up_the_screen() {
        let camera = OrthoCamera::new(1280, 720);
        let (right, up) = camera.ground_basis();

        let origin = to_ndc(&camera, Vec3::ZERO);
        let stepped_up = to_ndc(&camera, up);
        let stepped_right = to_ndc(&camera, right);

        assert!(stepped_up.y > origin.y, "up should raise NDC y");
        assert!((stepped_up.x - origin.x).abs() < 1e-5, "up should not drift sideways");

        assert!(stepped_right.x > origin.x, "right should raise NDC x");
        assert!((stepped_right.y - origin.y).abs() < 1e-5, "right should not drift vertically");
    }

    /// Under isometric projection the two screen axes are *not* the same length
    /// in world units — a step "up" covers less screen than a step "right",
    /// because the ground is foreshortened vertically by sin(35.26°). This
    /// pins the ratio, which is what makes the projection isometric rather
    /// than merely axonometric.
    #[test]
    fn the_projection_foreshortens_vertically() {
        let camera = OrthoCamera::new(1000, 1000);
        let (right, up) = camera.ground_basis();

        let origin = to_ndc(&camera, Vec3::ZERO);
        let dx = (to_ndc(&camera, right).x - origin.x).abs();
        let dy = (to_ndc(&camera, up).y - origin.y).abs();

        assert!((dy / dx - ISO_PITCH.sin()).abs() < 1e-4, "expected sin(pitch) foreshortening");
    }

    /// A minimised window reports zero height. Dividing by it yields NaN, which
    /// propagates through the whole matrix and blanks the screen with no error
    /// anywhere.
    #[test]
    fn a_zero_sized_viewport_does_not_poison_the_matrix() {
        let mut camera = OrthoCamera::new(1280, 720);
        camera.set_viewport(0, 0);
        assert!(camera.view_proj().is_finite());
    }

    #[test]
    fn zoom_is_clamped_at_both_ends() {
        let mut camera = OrthoCamera::new(1280, 720);
        for _ in 0..100 {
            camera.zoom_by(0.5);
        }
        assert_eq!(camera.zoom, MIN_ZOOM);

        for _ in 0..100 {
            camera.zoom_by(2.0);
        }
        assert_eq!(camera.zoom, MAX_ZOOM);
    }
}
