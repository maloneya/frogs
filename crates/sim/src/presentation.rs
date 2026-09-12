//! Read-only character facts consumed by presentation without becoming gameplay state.

use glam::{Vec2, Vec3};

use arpg_core::{Instance, InstanceSink};

use crate::{
    AttackStatus, EntityId, PLAYER_HALF_HEIGHT, PLAYER_NOSE_FORWARD, PLAYER_NOSE_SCALE,
    PLAYER_SCALE,
};

/// One frame's immutable view of the player.
///
/// Position and facing are interpolated presentation values. Displacement and
/// attack state come from the latest completed simulation tick. The snapshot
/// owns copies only, so animation cannot write anything back into the world.
#[derive(Clone, Copy, Debug)]
pub struct PlayerPresentation {
    id: EntityId,
    ground_position: Vec3,
    facing: f32,
    displacement: Vec2,
    attack: AttackStatus,
}

/// One frame's immutable view of an ordinary enemy.
///
/// The snapshot deliberately stops at simulation-owned facts. Animation phase,
/// clip selection, visual scale and tint remain presentation decisions.
#[derive(Clone, Copy, Debug)]
pub struct EnemyPresentation {
    id: EntityId,
    ground_position: Vec3,
    displacement: Vec2,
}

impl EnemyPresentation {
    pub(crate) fn new(id: EntityId, ground_position: Vec3, displacement: Vec2) -> Self {
        Self {
            id,
            ground_position,
            displacement,
        }
    }

    /// Stable simulation identity used to derive repeatable visual variation.
    #[must_use]
    pub fn id(self) -> EntityId {
        self.id
    }

    /// Interpolated asset origin on the ground plane.
    #[must_use]
    pub fn ground_position(self) -> Vec3 {
        self.ground_position
    }

    /// Actual ground-plane displacement over the latest completed tick.
    #[must_use]
    pub fn displacement(self) -> Vec2 {
        self.displacement
    }
}

impl PlayerPresentation {
    pub(crate) fn new(
        id: EntityId,
        ground_position: Vec3,
        facing: f32,
        displacement: Vec2,
        attack: AttackStatus,
    ) -> Self {
        Self {
            id,
            ground_position,
            facing,
            displacement,
            attack,
        }
    }

    /// Stable simulation identity represented by this snapshot.
    #[must_use]
    pub fn id(self) -> EntityId {
        self.id
    }

    /// Interpolated asset origin on the ground plane.
    #[must_use]
    pub fn ground_position(self) -> Vec3 {
        self.ground_position
    }

    /// Interpolated yaw using the engine's `+Z`-forward convention.
    #[must_use]
    pub fn facing(self) -> f32 {
        self.facing
    }

    /// Actual ground-plane displacement over the latest completed tick.
    #[must_use]
    pub fn displacement(self) -> Vec2 {
        self.displacement
    }

    /// Authoritative attack state derived from the in-flight swing.
    #[must_use]
    pub fn attack(self) -> AttackStatus {
        self.attack
    }

    /// Emits the original cube body when no character asset replaces it.
    pub fn extract_fallback(self, out: &mut InstanceSink<'_>) {
        let forward = Vec3::new(self.facing.sin(), 0.0, self.facing.cos());
        out.push(
            Instance::new(
                self.ground_position + Vec3::Y * PLAYER_SCALE.y + forward * PLAYER_NOSE_FORWARD,
                PLAYER_NOSE_SCALE,
                Vec3::new(0.95, 0.8, 0.4),
            )
            .with_yaw(self.facing),
        );
        out.push(
            Instance::new(
                self.ground_position + Vec3::Y * PLAYER_HALF_HEIGHT,
                PLAYER_SCALE,
                Vec3::new(0.10, 0.47, 0.88),
            )
            .with_yaw(self.facing),
        );
    }
}
