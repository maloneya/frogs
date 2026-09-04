//! The vocabulary both halves of the engine speak.
//!
//! Two vocabularies, in fact, and they are mirror images of each other:
//!
//! - [`Instance`] is what the simulation says *outward*, to the renderer —
//!   position, scale, colour, and nothing about what an entity is.
//! - [`Action`] is what the device layer says *inward*, to the simulation —
//!   intent, and nothing about which key produced it.
//!
//! Both live here rather than on either side, and this crate depends on neither
//! of them — nor on wgpu or winit, which is what keeps the simulation free of
//! the graphics stack *and* free of the window system.

mod input;
mod instance;

pub use input::{Action, ActionMask, Actions, InputState, MoveDir};
pub use instance::{Instance, InstanceBuffer, InstanceSink, MAX_INSTANCES};
