use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec3};

use arpg_core::MoveDir;

/// A true isometric camera.
///
/// "Isometric" is a specific claim, not a vibe: an orthographic projection at
/// 45° yaw and an elevation of `atan(1/√2)` ≈ 35.264°. At exactly that angle a
/// unit cube projects to a regular hexagon and its three visible faces have
/// equal area — hence *iso*-metric, equal measure. Any other elevation is some
/// other axonometric projection.
pub struct OrthoCamera {
    /// The point being looked at — *not* the player's position, but a value
    /// chasing it. [`OrthoCamera::follow`] is the only writer, so the smoothing
    /// cannot be bypassed by something that just wants to set the target.
    target: Vec3,
    /// The current look-ahead offset, itself smoothed. Held as state rather
    /// than recomputed because easing it is the whole point — see `follow`.
    lead: Vec3,
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

/// Seconds for the camera to close half the remaining distance to the player.
///
/// The single biggest feel knob in this file. Zero would pin the camera to the
/// character, which reads as *the world sliding around a stationary sprite*
/// rather than as movement — the character never budges within the frame, and
/// every jitter in its position becomes whole-screen motion. Too large and the
/// player outruns the view. Around an eighth of a second leaves the character
/// visibly leading inside the frame while never threatening to escape it.
const FOLLOW_HALF_LIFE: f32 = 0.12;

/// The same, for the look-ahead offset, and deliberately three times slower.
///
/// Look-ahead's failure mode is reversal: flick from right to left and a
/// rigid offset teleports the camera two lead-lengths across the screen while
/// the character has barely moved. Smoothing the offset on its own, slower
/// clock turns that whip into an ease.
const LEAD_HALF_LIFE: f32 = 0.35;

/// How far in front of the character the camera looks, in world units.
///
/// This is what separates an ARPG camera from a generic follow cam: you get
/// more of the screen in the direction you are heading, so what you are walking
/// into is visible before it reaches you. The offset is in *world* units, not
/// screen ones, so it reveals the same amount of world in every direction —
/// which is the axis threats live on. On screen the vertical lead therefore
/// looks shorter than the horizontal one, foreshortened by sin(35.26°), and
/// that is correct rather than a bug to compensate for.
const LOOK_AHEAD: f32 = 4.0;

/// Frame-rate independent exponential smoothing: moves `from` half of the
/// remaining distance to `to` every `half_life` seconds, **no matter how often
/// it is called**.
///
/// This is the one piece of maths worth getting right in any follow camera.
/// The tempting version is `from.lerp(to, 0.1)` once a frame, which keeps 90%
/// of the error *per frame* rather than per second: after one second that is
/// `0.9^60 ≈ 0.002` left at 60Hz but `0.9^144 ≈ 3e-7` at 144Hz, a camera some
/// thousands of times tighter purely because the machine is faster. Here that
/// would be worse than usual — pressing `V` to uncap the frame rate would
/// change how the game *feels*, corrupting the measurement it exists to take.
///
/// `2^(-dt/half_life)` composes exactly under subdivision, because
/// `2^(-a/h) · 2^(-b/h) = 2^(-(a+b)/h)`. Fifty steps of 10ms therefore land in
/// precisely the same place as one step of 500ms — which is the property the
/// naive form lacks, and what the test below pins.
fn damp(from: Vec3, to: Vec3, half_life: f32, dt: f32) -> Vec3 {
    to + (from - to) * (-dt / half_life).exp2()
}

impl OrthoCamera {
    /// A camera framing the origin, sized to the given viewport in pixels.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            target: Vec3::ZERO,
            lead: Vec3::ZERO,
            zoom: 16.0,
            aspect: aspect_of(width, height),
        }
    }

    /// Cuts straight to `focus`, with no smoothing and no lead.
    ///
    /// For spawning and teleporting — anywhere the character *discontinuously*
    /// changes position. Without it the camera eases in from wherever it was,
    /// so a player spawning away from the origin gets an unrequested swoop
    /// across the world, and a teleport gets a long slide through everything in
    /// between rather than a cut.
    pub fn snap_to(&mut self, focus: Vec3) {
        self.target = Vec3::new(focus.x, 0.0, focus.z);
        self.lead = Vec3::ZERO;
    }

    /// Chases `focus`, leading it in the direction of `heading`. Call once per
    /// rendered frame.
    ///
    /// Per *frame*, not per simulation tick, and that is deliberate: where the
    /// camera points is a presentation decision, not simulation state. Nothing
    /// downstream depends on it, no other system reads it, and smoothing it at
    /// display rate is what keeps the motion smooth at any refresh rate. When
    /// the fixed timestep lands, this call stays exactly where it is.
    ///
    /// Call it *after* stepping the world. Following last tick's position adds
    /// a frame of lag on top of the smoothing that is already there on purpose.
    pub fn follow(&mut self, focus: Vec3, heading: MoveDir, dt: f32) {
        // Track the ground position only. The character's height is not the
        // camera's business: once anything can jump, step up a stair or be
        // knocked into the air, tracking Y makes the whole view bob with it —
        // and a camera that moves when the player did not is nauseating in a
        // way a lagging one never is.
        let focus = Vec3::new(focus.x, 0.0, focus.z);

        self.lead = damp(self.lead, heading.as_vec3() * LOOK_AHEAD, LEAD_HALF_LIFE, dt);
        self.target = damp(self.target, focus + self.lead, FOLLOW_HALF_LIFE, dt);
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

    /// **The property the whole design exists for.** One coarse step and many
    /// fine steps covering the same wall-clock time must land in the same
    /// place, or the camera is tighter on a faster machine and the game feels
    /// different depending on the hardware — and, here, on whether `V` has been
    /// pressed.
    ///
    /// With `2^(-dt/h)` the two agree to floating-point noise. The naive
    /// per-frame lerp this replaces would be off by a factor of thousands.
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
