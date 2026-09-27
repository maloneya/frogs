//! Composable authored decisions and per-actor action execution.
//! Definitions are shared; progress, commitments and reservations belong to actors.

mod coordination;
mod definition;
mod runtime;

pub use coordination::MAX_COMBAT_COMMITMENTS;
pub use definition::{
    ActionDefinition, ActionTiming, ActorDefinition, ActorRole, BehaviorNode, Destination,
    LeapAction, MAX_ACTIONS, MAX_BEHAVIOR_DEPTH, MAX_BEHAVIOR_NODES, MoveAction, SwipeAction,
    TargetSelection,
};
pub(crate) use runtime::{ActionEffect, Actors};
pub use runtime::{ActionKind, ActionOutcome, ActionStatus};
