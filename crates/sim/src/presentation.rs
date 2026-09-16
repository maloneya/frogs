//! Read-only character facts consumed by presentation without becoming gameplay state.

use glam::{Vec2, Vec3};

use crate::{AttackStatus, EntityId, InteractionState};

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

}

/// Immutable physical facts for a non-damageable prop.
#[derive(Clone, Copy, Debug)]
pub struct PropPresentation {
    id: EntityId,
    ground_position: Vec3,
    interaction: Option<InteractionState>,
}

impl PropPresentation {
    pub(crate) fn new(id: EntityId, ground_position: Vec3, interaction: Option<InteractionState>) -> Self {
        Self { id, ground_position, interaction }
    }

    /// Stable identity, independent of dense storage order.
    #[must_use]
    pub fn id(self) -> EntityId { self.id }

    /// Interpolated asset origin on the ground plane, in metres.
    #[must_use]
    pub fn ground_position(self) -> Vec3 { self.ground_position }

    /// Authoritative interaction state; absent for non-interactable props.
    #[must_use]
    pub fn interaction(self) -> Option<InteractionState> { self.interaction }
}

/// Authoritative collision geometry at the last completed tick, without interpolation.
#[derive(Clone, Copy, Debug)]
pub struct CollisionDisc {
    id: EntityId,
    centre: Vec3,
    radius: f32,
}

impl CollisionDisc {
    pub(crate) fn new(id: EntityId, centre: Vec3, radius: f32) -> Self { Self { id, centre, radius } }

    /// Stable identity of the physical body.
    #[must_use]
    pub fn id(self) -> EntityId { self.id }

    /// Ground-plane centre used by collision detection.
    #[must_use]
    pub fn centre(self) -> Vec3 { self.centre }

    /// Radius used by collision detection, in metres.
    #[must_use]
    pub fn radius(self) -> f32 { self.radius }
}

/// One committed attack sample placed at the completed tick's player pose.
#[derive(Clone, Copy, Debug)]
pub struct AttackDisc {
    centre: Vec3,
    radius: f32,
    sample: usize,
}

impl AttackDisc {
    /// Maximum number of samples a committed swing can contain.
    pub const MAX_SAMPLES: usize = crate::ResolvedAttack::MAX_ACTIVE_TICKS as usize;

    pub(crate) fn new(centre: Vec3, radius: f32, sample: usize) -> Self {
        Self { centre, radius, sample }
    }

    /// Ground-plane centre in metres.
    #[must_use]
    pub fn centre(self) -> Vec3 { self.centre }

    /// Physical hitbox radius in metres.
    #[must_use]
    pub fn radius(self) -> f32 { self.radius }

    /// Zero-based tick within the committed active window.
    #[must_use]
    pub fn sample(self) -> usize { self.sample }
}
