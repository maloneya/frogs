//! Gameplay diagnostic vocabulary, independent of physical mechanism events.
use crate::{DoorState, EncounterState};
use arpg_sim::SceneId;
use serde::Deserialize;

/// Diagnostic transitions; no gameplay logic reads these events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameplayEvent {
    /// Accepted gameplay tuning edit between ticks.
    TuningChanged {
        /// Attribute edited.
        field: crate::TuningField,
        /// Accepted value.
        value: f32,
    },
    /// Selection for subsequent player actions; committed actions keep their values.
    AttackProfileChanged {
        /// New authored ability.
        profile: crate::AttackProfile,
    },
    /// Player melee action committed.
    Swung,
    /// Held player channel began.
    ChannelStarted,
    /// Held player channel ended.
    ChannelStopped,
    /// Committed melee hitbox became active.
    HitboxOpened,
    /// Committed melee hitbox became inactive.
    HitboxClosed,

    /// Phase or progress changed.
    Progress {
        /// Owning scene.
        scene: SceneId,
        /// Authored encounter index.
        index: usize,
        /// State after the transition.
        state: EncounterState,
    },
    /// Chest changed state.
    Chest {
        /// Owning scene.
        scene: SceneId,
        /// Authored chest index.
        index: usize,
        /// New chest state.
        state: crate::ChestState,
    },
    /// An encounter changed a door's latch. Physical breaches are engine
    /// transitions, traced as `arpg_sim::Event::LockBroken`.
    Door {
        /// Owning scene.
        scene: SceneId,
        /// Authored door index.
        index: usize,
        /// New door state.
        state: DoorState,
    },
}
/// Gameplay diagnostic categories; physical categories remain with sim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum GameplayEventKind {
    /// A runtime tuning edit was accepted.
    TuningChanged,
    /// The selected ability changed.
    AttackProfileChanged,
    /// A melee action committed.
    Swung,
    /// A held channel began.
    ChannelStarted,
    /// A held channel ended.
    ChannelStopped,
    /// A committed melee hitbox became active.
    HitboxOpened,
    /// A committed melee hitbox became inactive.
    HitboxClosed,
    /// Encounter phase or progress changed.
    Progress,
    /// A chest changed state.
    Chest,
    /// An encounter changed a door latch.
    Door,
}
impl GameplayEvent {
    /// Category used for bounded selected-event capture.
    pub fn kind(self) -> GameplayEventKind {
        match self {
            Self::TuningChanged { .. } => GameplayEventKind::TuningChanged,
            Self::AttackProfileChanged { .. } => GameplayEventKind::AttackProfileChanged,
            Self::Swung => GameplayEventKind::Swung,
            Self::ChannelStarted => GameplayEventKind::ChannelStarted,
            Self::ChannelStopped => GameplayEventKind::ChannelStopped,
            Self::HitboxOpened => GameplayEventKind::HitboxOpened,
            Self::HitboxClosed => GameplayEventKind::HitboxClosed,
            Self::Progress { .. } => GameplayEventKind::Progress,
            Self::Chest { .. } => GameplayEventKind::Chest,
            Self::Door { .. } => GameplayEventKind::Door,
        }
    }
}
impl std::fmt::Display for GameplayEvent {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TuningChanged { field, value } => {
                write!(out, "tuning {}={value}", field.attribute().key)
            }
            Self::AttackProfileChanged { profile } => {
                write!(out, "attack profile={}", profile.label())
            }
            Self::Swung => write!(out, "swung"),
            Self::ChannelStarted => write!(out, "channel started"),
            Self::ChannelStopped => write!(out, "channel stopped"),
            Self::HitboxOpened => write!(out, "hitbox opened"),
            Self::HitboxClosed => write!(out, "hitbox closed"),

            Self::Progress {
                scene,
                index,
                state,
            } => write!(
                out,
                "encounter scene={scene} index={index} phase={:?} defeated={} remaining={}",
                state.phase, state.defeated, state.remaining_ticks
            ),
            Self::Chest {
                scene,
                index,
                state,
            } => write!(out, "chest scene={scene} index={index} state={state:?}"),
            Self::Door {
                scene,
                index,
                state,
            } => write!(out, "door scene={scene} index={index} state={state:?}"),
        }
    }
}
