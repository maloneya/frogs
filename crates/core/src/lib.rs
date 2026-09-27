//! Shared input, geometry and reporting vocabulary, independent of devices and gameplay.
//!
//! # World units
//!
//! World distances are metres, speeds metres/second, accelerations metres/second²,
//! angles radians. X/Z span the ground; Y is height. Mesh scale is dimensionless;
//! visible geometry, collision and attack shapes have independent dimensions.
//! Overlay coordinates are pixels. Convert external units at import boundaries.
//! These conventions are not enforced by dimensional types.

mod colour;
mod lighting;
mod input;
mod instance;
mod intent;
mod report;
mod smoothing;

pub use colour::{LinearRgb, LinearRgba, Srgb};
pub use lighting::{EnvironmentLight, PointLight, MAX_POINT_LIGHTS};
pub use input::{Action, ActionMask, Actions, InputState};
pub use instance::{Instance, InstanceBuffer, InstanceSink, MAX_INSTANCES};
pub use intent::{AimDir, Intent, MoveDir};
pub use report::Report;
pub use smoothing::{damp, damp_vec3};

/// Shared fixed camera orientation for rendering and headless authoring views.
pub fn isometric_eye_direction() -> glam::Vec3 {
    let yaw = std::f32::consts::FRAC_PI_4;
    let pitch = 0.615_479_7_f32;
    glam::Vec3::new(pitch.cos() * yaw.cos(), pitch.sin(), pitch.cos() * yaw.sin())
}

/// Projects world ground offsets to screen right/up, before pan and zoom.
pub fn isometric_ground_projection() -> [[f32; 2]; 2] {
    let forward = -isometric_eye_direction().normalize();
    let right = forward.cross(glam::Vec3::Y).normalize();
    let up = right.cross(forward);
    [[right.x, right.z], [up.x, up.z]]
}
