//! Capability splits for deterministic callers outside the engine schedule.
//! The constructor can borrow World; the returned types cannot reach it.

use crate::pass::source::Sources;
use crate::{EntityId, InteractionState, SourceId, SourceState, TraceSink, World};

/// Read-only physical facts for explicit gameplay-selected actors and targets.
#[derive(Clone, Copy)]
pub struct ActorView<'a> {
    bodies: &'a crate::Bodies,
    walls: &'a crate::wall::Walls,
}
impl ActorView<'_> {
    /// Position of a live generational identity.
    pub fn position(&self, id: EntityId) -> Option<glam::Vec2> {
        self.bodies.pos_of(id)
    }
    /// Current physical centre velocity of a live identity.
    pub fn velocity(&self, id: EntityId) -> Option<glam::Vec2> {
        self.bodies.physics.get(id).map(crate::Motion::velocity)
    }
    /// Health is independent of hostility; absent membership means immune.
    pub fn health(&self, id: EntityId) -> Option<u8> {
        self.bodies.health.get(id)
    }
    /// Initial health capacity, used by presentation without assuming a creature type.
    pub fn max_health(&self, id: EntityId) -> Option<u8> {
        self.bodies.health.maximum(id)
    }
    /// A live identity whose health, if present, has not reached zero.
    pub fn alive(&self, id: EntityId) -> bool {
        self.bodies.slots.index(id).is_some() && self.health(id) != Some(0)
    }
    /// Producer eligibility preserves the protected player's existing play-at-zero
    /// policy; defeated nonplayer actors cannot finish an action.
    pub fn can_act(&self, id: EntityId) -> bool {
        self.bodies.slots.index(id).is_some()
            && (id == self.player_id() || self.health(id) != Some(0))
    }
    /// Whether an existing health member has reached zero before removal.
    pub fn defeated(&self, id: EntityId) -> bool {
        self.health(id) == Some(0)
    }
    /// Whether this live actor accepts powered motion. Hinged panels do not.
    pub fn movable(&self, id: EntityId) -> bool {
        self.can_act(id)
            && self
                .bodies
                .physics
                .get(id)
                .is_some_and(|m| m.inverse_mass() > 0.0)
            && self.bodies.physics.panels.index(id).is_none()
    }
    /// Stable identity of the persistent player body.
    pub fn player_id(&self) -> EntityId {
        self.bodies.slots.ids()[0]
    }
    /// Shared collision footprint for a live explicit target.
    pub fn radius(&self, id: EntityId) -> Option<f32> {
        self.bodies.slots.index(id).map(crate::body_radius)
    }
    /// Bodies touched by a disc, in stable storage order, with contact points on
    /// their boundary facing the disc centre. Cover uses the attack origin.
    /// Health and gameplay role do not affect physical contact membership.
    pub fn disc_contacts(
        &self,
        origin: glam::Vec2,
        centre: glam::Vec2,
        radius: f32,
    ) -> impl Iterator<Item = (EntityId, glam::Vec2)> + '_ {
        self.bodies
            .slots
            .ids()
            .iter()
            .copied()
            .enumerate()
            .filter_map(move |(row, id)| {
                if !origin.is_finite() || !centre.is_finite() || !radius.is_finite() || radius < 0.0
                {
                    return None;
                }
                let position = self.bodies.pos[row];
                let body_radius = crate::body_radius(row);
                let point =
                    self.bodies
                        .physics
                        .disc_contact(id, position, body_radius, centre, radius)?;
                if self
                    .bodies
                    .physics
                    .obstructs(self.walls, origin, point, Some(id))
                {
                    return None;
                }
                Some((id, point))
            })
    }
    /// Current interaction state for a live endpoint.
    pub fn interaction(&self, id: EntityId) -> Option<InteractionState> {
        self.bodies.interactions.get(id)
    }
    /// Exact shared admission for a live actor activating a ready endpoint.
    /// Decisions and final effects use the same reach, health and cover rules.
    pub fn can_interact(&self, actor: EntityId, target: EntityId) -> bool {
        crate::pass::interact::can_activate(
            actor,
            target,
            &self.bodies.interactions,
            &self.bodies.slots,
            &self.bodies.pos,
            &self.bodies.health,
            self.walls,
            &self.bodies.physics,
        )
    }
    /// Available endpoint facts in capability storage order; selection must break ties by identity.
    pub fn interactions(
        &self,
    ) -> impl Iterator<Item = (EntityId, glam::Vec2, InteractionState)> + '_ {
        self.bodies
            .interactions
            .ids()
            .iter()
            .copied()
            .filter_map(|id| Some((id, self.position(id)?, self.interaction(id)?)))
    }
    /// Shared wall geometry rejects nonfinite or obstructed segments.
    pub fn clear_path(&self, start: glam::Vec2, end: glam::Vec2) -> bool {
        start.is_finite()
            && end.is_finite()
            && !self.bodies.physics.obstructs(self.walls, start, end, None)
    }
}
impl World {
    /// Narrow observation used both between ticks and by the action schedule.
    pub fn actor_view(&self) -> ActorView<'_> {
        ActorView {
            bodies: &self.bodies,
            walls: &self.scenes.walls,
        }
    }
}

/// Between-tick authority over static props. Cannot move bodies, deal damage,
/// create enemies or remove a movable body (including the player).
pub struct GameplayAccess<'a> {
    bodies: &'a mut crate::Bodies,
    sources: &'a mut Sources,
    trace: TraceSink<'a>,
}

impl GameplayAccess<'_> {
    /// Current hinged physical state for a live body.
    pub fn panel(&self, id: EntityId) -> Option<crate::PanelState> {
        self.bodies.physics.panel(id)
    }
    /// Release an intact latch without deleting or moving its panel.
    pub fn release_latch(&mut self, id: EntityId) -> bool {
        self.bodies.physics.panels.release(id)
    }

    /// Observe a source without access to its spawn requests.
    pub fn source(&self, id: SourceId) -> Option<SourceState> {
        self.sources.state(id)
    }

    /// Enable or pause a live source; never cancels accepted work.
    pub fn set_source(&mut self, id: SourceId, enabled: bool) -> bool {
        self.sources.set_enabled(id, enabled, self.trace.reborrow())
    }

    /// Observe interaction state, including movable activation endpoints.
    pub fn interaction(&self, id: EntityId) -> Option<InteractionState> {
        self.bodies.interactions.get(id)
    }

    /// Whether this identity still names an immovable prop.
    pub fn contains(&self, id: EntityId) -> bool {
        self.bodies
            .physics
            .get(id)
            .is_some_and(|motion| motion.inverse_mass() == 0.0)
    }

    /// Grants interaction once to a live static prop. Existing activation is preserved.
    pub fn enable_interaction(&mut self, id: EntityId) -> bool {
        if !self.contains(id) {
            return false;
        }
        self.bodies.interactions.grant(id);
        true
    }
}

impl World {
    /// Restricted gameplay operations and lossless completed-tick outcomes.
    pub fn encounter_access(&mut self) -> (GameplayAccess<'_>, &[crate::Defeat]) {
        (
            GameplayAccess {
                bodies: &mut self.bodies,
                sources: &mut self.sources,
                trace: self.trace.sink(self.tick),
            },
            &self.defeats,
        )
    }
}
