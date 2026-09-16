//! The vocabulary both halves of the engine speak.
//!
//! Two vocabularies, in fact, and they are mirror images of each other:
//!
//! - [`Instance`] is what presentation says *outward*, to the renderer —
//!   position, scale, colour, and nothing about what an entity is.
//! - [`Action`] is what the device layer says *inward*, and [`Intent`] is what
//!   that becomes once the camera has resolved screen to world. Neither names a
//!   key; only `Intent` reaches the simulation.
//!
//! Both live here rather than on either side, and this crate depends on neither
//! of them — nor on wgpu or winit, which is what keeps the simulation free of
//! the graphics stack *and* free of the window system.
//!
//! [`damp`] is here on the same terms: it is behaviour rather than vocabulary,
//! but it depends on neither side and is needed by both, and putting it in reach
//! is what stops the next person writing the frame-rate-dependent lerp instead.
//! That pair — *needed by both, beholden to neither* — is the bar for anything
//! else that wants to live here.
//!
//! # World units
//!
//! **One world unit is one metre.** World positions, distances, body dimensions,
//! collision radii and attack offsets use this same scale. Linear speeds are
//! metres per second; linear accelerations are metres per second squared.
//! Durations expressed in seconds use seconds; tick counts remain tick counts.
//! Angles are radians. X and Z span the ground plane; Y is height.
//!
//! Mesh scale is a dimensionless multiplier: a unit cube scaled by 0.5 has
//! sides of 0.5 metres. A mesh's visible dimensions, its collision shape and
//! its attack shape are separate choices, all measured with the same ruler.
//! Camera projection converts world space to screen space; overlay coordinates
//! are pixels. Zoom never changes simulation distances.
//!
//! Scene files, harness commands and state reports use these units for spatial
//! values too. Convert external measurements at import, rather than introducing
//! a second scale inside the engine. This is a fixed convention, not a runtime
//! setting, and choosing metres does not require realistic proportions or speed.
//!
//! This contract is documented, not enforced by dimensional types: ordinary
//! floats and vectors do not distinguish metres from pixels or seconds.

mod input;
mod instance;
mod intent;
mod report;
mod smoothing;

pub use input::{Action, ActionMask, Actions, InputState};
pub use instance::{Instance, InstanceBuffer, InstanceSink, MAX_INSTANCES};
pub use intent::{Intent, MoveDir};
pub use report::Report;
pub use smoothing::{damp, damp_vec3};
