use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec3};

use arpg_core::{damp_vec3, MoveDir};

/// Orthographic camera at 45° yaw and atan(1/√2) elevation.
pub struct OrthoCamera {
    /// Smoothed ground focus.
    target: Vec3,
    /// Smoothed look-ahead offset.
    lead: Vec3,
    /// Half-height of the view volume, in world units. Smaller = zoomed in.
    zoom: f32,
    aspect: f32,
}

/// View half-height bounds; zero would make the projection degenerate.
const MIN_ZOOM: f32 = 2.0;
const MAX_ZOOM: f32 = 400.0;
const _: () = assert!(MIN_ZOOM > 0.0, "a zoom of zero collapses the view volume");
const _: () = assert!(MAX_ZOOM > MIN_ZOOM);

/// A zero-sized window is real — minimising produces one — and dividing by it
/// yields a NaN that propagates silently through the whole matrix.
fn aspect_of(width: u32, height: u32) -> f32 {
    width.max(1) as f32 / height.max(1) as f32
}


/// How far back the camera sits. Under orthographic projection this has *no*
/// effect on apparent size — there's no perspective divide — so it only has to
/// be large enough that the near plane never clips the scene.
const DISTANCE: f32 = 250.0;
const _: () = assert!(DISTANCE > 0.0);

/// Follow half-life in seconds; balances visible player movement against camera lag.
const FOLLOW_HALF_LIFE: f32 = 0.12;

const _: () = assert!(FOLLOW_HALF_LIFE > 0.0, "a non-positive half-life diverges to NaN");

/// Slower look-ahead easing prevents a camera jump when direction reverses.
const LEAD_HALF_LIFE: f32 = 0.35;

const _: () = assert!(LEAD_HALF_LIFE > 0.0);
const _: () = assert!(
    LEAD_HALF_LIFE > FOLLOW_HALF_LIFE,
    "the lead must ease slower than the follow, or reversing whips the camera"
);

/// Look-ahead distance in world metres; projection foreshortens vertical screen lead.
const LOOK_AHEAD: f32 = 4.0;
const _: () = assert!(LOOK_AHEAD >= 0.0);

impl OrthoCamera {
    /// A camera framing the origin, sized to the given viewport in pixels.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            target: Vec3::ZERO,
            lead: Vec3::ZERO,
            zoom: 16.0 * 0.67,
            aspect: aspect_of(width, height),
        }
    }

    /// Cuts to `focus` without smoothing or lead, for spawning and teleporting.
    pub fn snap_to(&mut self, focus: Vec3) {
        self.target = Vec3::new(focus.x, 0.0, focus.z);
        self.lead = Vec3::ZERO;
    }

    /// Current smoothed focus.
    pub fn target(&self) -> Vec3 {
        self.target
    }

    /// Vertical extent of the orthographic view in world units, for observations.
    pub fn view_height(&self) -> f32 { 2.0 * self.zoom }

    /// Follows focus with look-ahead. Call once per rendered frame after stepping Game.
    pub fn follow(&mut self, focus: Vec3, heading: MoveDir, dt: f32) {
        // Ignore height so vertical character motion does not bob the camera.
        let focus = Vec3::new(focus.x, 0.0, focus.z);

        self.lead = damp_vec3(self.lead, heading.as_vec3() * LOOK_AHEAD, LEAD_HALF_LIFE, dt);
        self.target = damp_vec3(self.target, focus + self.lead, FOLLOW_HALF_LIFE, dt);
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
        let dir = arpg_core::isometric_eye_direction();
        self.target + dir * DISTANCE
    }

    /// Screen right/up projected onto the ground as perpendicular unit directions.
    pub fn ground_basis(&self) -> (Vec3, Vec3) {
        let forward = (self.target - self.eye()).normalize();

        // `forward × Y` already lies in the XZ plane, so "right" needs no
        // flattening. Crossing back the other way recovers the ground-plane
        // component of forward — i.e. straight away from the camera, which is
        // what "up the screen" means on the floor.
        let right = forward.cross(Vec3::Y).normalize();
        (right, Vec3::Y.cross(right))
    }

    fn view_matrix(&self) -> Mat4 {
        look_at_mat4(self.eye(), self.target, Vec3::Y)
    }

    /// Projects a physical viewport pixel onto the ground using this camera's
    /// current view. Invalid or zero-sized viewports are rejected.
    pub fn ground_point(&self, pixel: glam::Vec2, width: u32, height: u32) -> Result<Vec3, &'static str> {
        if !pixel.is_finite() || width == 0 || height == 0 {
            return Err("cursor needs finite pixels and a nonzero viewport");
        }
        let ndc = glam::Vec2::new(pixel.x / width as f32 * 2.0 - 1.0, 1.0 - pixel.y / height as f32 * 2.0);
        let inverse = self.view_proj().inverse();
        let near = inverse.project_point3(ndc.extend(0.0));
        let far = inverse.project_point3(ndc.extend(1.0));
        let ray = far - near;
        let point = near - ray * (near.y / ray.y);
        if !point.is_finite() { return Err("cursor ray does not intersect the ground"); }
        Ok(Vec3::new(point.x, 0.0, point.z))
    }

    /// The combined view-projection matrix, as uploaded to the shader.
    pub fn view_proj(&self) -> Mat4 {
        let view = self.view_matrix();

        let half_h = self.zoom;
        let half_w = self.zoom * self.aspect;

        // wgpu uses Y-up, Z in [0, 1], matching glam’s DirectX projection convention.
        let proj = directx::orthographic(-half_w, half_w, -half_h, half_h, 0.1, DISTANCE * 2.0);

        proj * view
    }
}

/// Per-view GPU bindings: the camera matrix and the scene's bounded light list.
pub(crate) struct CameraBinding {
    buffer: wgpu::Buffer,
    light: wgpu::Buffer,
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

        let light = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point light uniform"),
            size: size_of::<crate::lighting::LightUniform>() as u64,
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
            }, wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<crate::lighting::LightUniform>() as u64),
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
            }, wgpu::BindGroupEntry {
                binding: 1,
                resource: light.as_entire_binding(),
            }],
        });

        Self { buffer, light, layout, bind_group }
    }

    pub(crate) fn upload_lights(&self, queue: &wgpu::Queue, lights: &[crate::PointLight], environment: arpg_core::EnvironmentLight) {
        queue.write_buffer(&self.light, 0, bytemuck::bytes_of(&crate::lighting::LightUniform::new(lights, environment)));
    }

    /// Uploads the camera matrix for this view.
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

        assert!((dy / dx - arpg_core::isometric_eye_direction().y).abs() < 1e-4, "expected sin(pitch) foreshortening");
    }

    #[test]
    fn authoring_ground_projection_matches_rendering_after_pan_and_zoom() {
        let mut camera = OrthoCamera::new(1280, 720);
        camera.snap_to(Vec3::new(12.0, 0.0, -7.0));
        camera.zoom_by(1.7);
        let projection = arpg_core::isometric_ground_projection();
        let origin = to_ndc(&camera, camera.target());
        for offset in [Vec3::X, Vec3::Z, Vec3::new(-3.0, 0.0, 9.0)] {
            let rendered = to_ndc(&camera, camera.target() + offset) - origin;
            let right = projection[0][0] * offset.x + projection[0][1] * offset.z;
            let up = projection[1][0] * offset.x + projection[1][1] * offset.z;
            assert!((rendered.x * camera.zoom * camera.aspect - right).abs() < 1e-4);
            assert!((rendered.y * camera.zoom - up).abs() < 1e-4);
        }
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
    fn following_is_frame_rate_independent() {
        let focus = Vec3::new(30.0, 0.0, -12.0);

        let mut coarse = OrthoCamera::new(1280, 720);
        coarse.follow(focus, MoveDir::NONE, 0.5);

        let mut fine = OrthoCamera::new(1280, 720);
        for _ in 0..50 {
            fine.follow(focus, MoveDir::NONE, 0.01);
        }

        assert!(
            (coarse.target - fine.target).length() < 1e-4,
            "{} vs {}",
            coarse.target,
            fine.target
        );
    }

    /// A half-life is a claim with a number attached, so check the number: one
    /// half-life of elapsed time closes exactly half the gap.
    #[test]
    fn one_half_life_closes_half_the_distance() {
        let mut camera = OrthoCamera::new(1280, 720);
        camera.follow(Vec3::new(10.0, 0.0, 0.0), MoveDir::NONE, FOLLOW_HALF_LIFE);
        assert!((camera.target.x - 5.0).abs() < 1e-3, "got {}", camera.target.x);
    }

    /// Standing still, the camera must arrive at the character and stop —
    /// including letting the look-ahead decay away, so idling does not leave
    /// the character parked off-centre.
    #[test]
    fn a_stationary_player_ends_up_centred() {
        let mut camera = OrthoCamera::new(1280, 720);
        let focus = Vec3::new(12.0, 0.6, -7.0);

        // Walk east for a while, then stop and let it settle.
        for _ in 0..200 {
            camera.follow(focus, MoveDir::new(Vec3::X), 1.0 / 60.0);
        }
        assert!(camera.target.x > focus.x, "should be leading east while moving");

        for _ in 0..400 {
            camera.follow(focus, MoveDir::NONE, 1.0 / 60.0);
        }
        assert!((camera.target - Vec3::new(focus.x, 0.0, focus.z)).length() < 1e-3);
    }

    /// The character's height is not the camera's business — otherwise the view
    /// bobs the first time anything jumps or is knocked upward.
    #[test]
    fn vertical_movement_is_ignored() {
        let mut camera = OrthoCamera::new(1280, 720);
        for _ in 0..200 {
            camera.follow(Vec3::new(0.0, 50.0, 0.0), MoveDir::NONE, 1.0 / 60.0);
        }
        assert!(camera.target.y.abs() < 1e-4, "camera rose to {}", camera.target.y);
    }

    /// Exponential damping approaches its target and never passes it. A spring
    /// would overshoot and bounce, which on a camera reads as seasickness.
    #[test]
    fn the_camera_never_overshoots() {
        let mut camera = OrthoCamera::new(1280, 720);
        let focus = Vec3::new(20.0, 0.0, 0.0);
        let mut previous = camera.target.x;

        for _ in 0..300 {
            camera.follow(focus, MoveDir::NONE, 1.0 / 60.0);
            assert!(camera.target.x >= previous, "moved backwards");
            assert!(camera.target.x <= focus.x, "overshot to {}", camera.target.x);
            previous = camera.target.x;
        }
    }

    /// Reversing direction must ease the lead across rather than whip it. One
    /// frame after a reversal the offset should barely have moved, and it must
    /// never jump further than the full lead in a single frame.
    #[test]
    fn reversing_direction_does_not_whip_the_lead() {
        let mut camera = OrthoCamera::new(1280, 720);
        let focus = Vec3::ZERO;

        for _ in 0..200 {
            camera.follow(focus, MoveDir::new(Vec3::X), 1.0 / 60.0);
        }
        let settled = camera.lead;
        assert!(settled.x > 0.0);

        camera.follow(focus, MoveDir::new(Vec3::NEG_X), 1.0 / 60.0);
        let moved = (camera.lead - settled).length();
        assert!(moved < LOOK_AHEAD * 0.1, "lead jumped {moved} in one frame");
    }

    /// Spawning away from the origin must be a cut, not a swoop.
    #[test]
    fn snapping_arrives_immediately_and_clears_the_lead() {
        let mut camera = OrthoCamera::new(1280, 720);
        for _ in 0..200 {
            camera.follow(Vec3::ZERO, MoveDir::new(Vec3::X), 1.0 / 60.0);
        }
        assert!(camera.lead.length() > 0.0, "should have built up a lead");

        camera.snap_to(Vec3::new(40.0, 9.0, -25.0));
        assert_eq!(camera.target, Vec3::new(40.0, 0.0, -25.0));
        assert_eq!(camera.lead, Vec3::ZERO);
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
