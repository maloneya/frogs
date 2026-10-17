//! Player presentation composes physical pose with gameplay ability state.
use crate::AttackStatus;
use arpg_sim::EntityId;
use glam::{Vec2, Vec3};
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
