//! Deterministic gameplay driven by both app and scenario.
//!
//! Source control runs before the engine tick through restricted interaction
//! and enablement capabilities. Game owns resolved gameplay relationships,
//! atomic scene replacement, and the complete restart snapshot. Physical
//! mechanisms and resources remain in sim; file decoding remains in content.
//! No ordinary gameplay pass receives mutable access to Game or World.

mod game;
mod scene;
mod torch;
pub use torch::TorchSpec;
mod source_control;
mod enemy_attack;

pub use game::Game;
pub use scene::{GameScene, RestartError, SceneError};
pub use source_control::{ControlEvent, ControlPhase, ControlState, SourceControlSpec};
pub use enemy_attack::{EnemyAttackSpec, PLAYER_MAX_HEALTH};

mod door;
mod encounter;
pub use door::{DoorSpec, DoorState};
pub use encounter::{DefeatObjective, EncounterAction, EncounterEvent, EncounterObjective, EncounterPhase, EncounterSpec, EncounterStart, EncounterState, MAX_ENCOUNTERS};

mod chest;
pub use chest::{ChestSpec, ChestState, MAX_CHESTS};
