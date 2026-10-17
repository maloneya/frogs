//! Read-only character facts consumed by presentation without becoming gameplay state.

use glam::{Vec2, Vec3};

use crate::{EntityId, InteractionState};

/// One frame's immutable view of a nonplayer character.
///
/// The snapshot deliberately stops at simulation-owned facts. Animation phase,
/// clip selection, visual scale and tint remain presentation decisions.
#[derive(Clone, Copy, Debug)]
pub struct CharacterPresentation {
    id: EntityId,
    ground_position: Vec3,
    displacement: Vec2,
}

impl CharacterPresentation {
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

/// Immutable physical facts for a body without character presentation.
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

/// One committed attack sample placed at its producer's completed-tick pose.
#[derive(Clone, Copy, Debug)]
pub struct AttackDisc {
    centre: Vec3,
    radius: f32,
    sample: usize,
}

impl AttackDisc {
    /// Maximum number of samples a committed swing can contain.
    pub const MAX_SAMPLES: usize = crate::ResolvedAttack::MAX_ACTIVE_TICKS as usize;

    /// Place one committed sample for read-only presentation.
    pub fn new(centre: Vec3, radius: f32, sample: usize) -> Self {
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
