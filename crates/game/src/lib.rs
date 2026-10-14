//! Deterministic gameplay driven by both app and scenario.
//!
//! Source control runs before the engine tick through restricted interaction
//! and enablement capabilities. Game owns resolved gameplay relationships,
//! atomic scene replacement, and the complete restart snapshot. Physical
//! mechanisms and resources remain in sim; file decoding remains in content.
//! No ordinary gameplay pass receives mutable access to Game or World.

// New gameplay states require an explicit transition decision.
#![deny(clippy::wildcard_enum_match_arm)]

mod scene_edit;
mod environment;
pub use environment::EnvironmentSpec;
mod game;
mod scene;
mod torch;
pub use torch::TorchSpec;
mod behavior;
mod population;
pub use behavior::{ActorDefinition, ActorRole, ActionDefinition, ActionKind, ActionStatus, ActionOutcome, BehaviorNode, Destination, TargetSelection, MoveAction, SwipeAction, LeapAction, ActionTiming, MAX_ACTIONS, MAX_BEHAVIOR_NODES, MAX_BEHAVIOR_DEPTH, MAX_COMBAT_COMMITMENTS};
pub use population::{Scene, Placed, BodyGrid, SourceSpec, Source};
pub use arpg_sim::PLAYER_MAX_HEALTH;

pub use game::Game;
pub use scene::{GameScene, RestartError, SceneError};

mod door;
mod encounter;
pub use door::{DoorPanel, DoorPose, DoorSpec, DoorState};
pub use encounter::{DefeatObjective, EncounterAction, EncounterEvent, EncounterObjective, EncounterPhase, EncounterSpec, EncounterStart, EncounterState, MAX_ENCOUNTERS};

mod chest;
pub use chest::{ChestSpec, ChestState, MAX_CHESTS};


mod tree;
pub use tree::{TreeKind, TreePose, TreeSpec, TreeState, MAX_TREES};

mod corpse;
pub use corpse::{Corpse, MAX_CORPSES, CORPSE_LIFETIME_TICKS};
