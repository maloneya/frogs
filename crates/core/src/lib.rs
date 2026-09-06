//! The vocabulary both halves of the engine speak.
//!
//! Two vocabularies, in fact, and they are mirror images of each other:
//!
//! - [`Instance`] is what the simulation says *outward*, to the renderer —
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
