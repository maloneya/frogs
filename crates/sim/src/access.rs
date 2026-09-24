//! Capability splits for deterministic callers outside the engine schedule.
//! The constructor can borrow World; the returned types cannot reach it.

use crate::{EntityId, InteractionState, PLAYER_ENEMY_CONTACT_DISTANCE, SourceId, SourceState, TraceSink, World};
use crate::pass::source::Sources;

/// Read-only physical facts needed by a gameplay-owned melee relationship.
/// The caller supplies a stable attacker identity and cannot inspect or move
/// arbitrary body storage.
pub struct MeleeView<'a> {
    bodies: &'a crate::Bodies,
    seekers: &'a crate::Members,
    walls: &'a crate::wall::Walls,
}

impl MeleeView<'_> {
    /// Whether this live seeker has reached the player's collision boundary.
    /// `None` means the identity is stale or no longer has seeking capability.
    #[must_use]
    pub fn reached_player(&self, id: EntityId) -> Option<bool> {
        if !self.seekers.contains(id) { return None; }
        let row = self.bodies.slots.index(id)?;
        let reach = PLAYER_ENEMY_CONTACT_DISTANCE;
        Some(self.bodies.pos[0].distance_squared(self.bodies.pos[row]) <= reach * reach
            && !self.walls.obstructs(self.bodies.pos[row], self.bodies.pos[0]))
    }
}

impl World {
    /// Restricts gameplay melee checks to stable identity and shared geometry.
    #[must_use]
    pub fn melee_view(&self) -> MeleeView<'_> {
        MeleeView { bodies: &self.bodies, seekers: &self.seekers, walls: &self.scenes.walls }
    }
}

/// Between-tick authority over static props. Cannot move bodies, deal damage,
/// create enemies or remove a movable body (including the player).
pub struct GameplayAccess<'a> {
    bodies: &'a mut crate::Bodies,
    seekers: &'a mut crate::Members,
    scenes: &'a mut crate::Scenes,
    sources: &'a mut Sources,
    trace: TraceSink<'a>,
}

impl GameplayAccess<'_> {
    /// Observe a source without access to its spawn requests.
    pub fn source(&self, id: SourceId) -> Option<SourceState> { self.sources.state(id) }

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
        self.bodies.physics.get(id).is_some_and(|motion| motion.inverse_mass() == 0.0)
    }

    /// Grants interaction once to a live static prop. Existing activation is preserved.
    pub fn enable_interaction(&mut self, id: EntityId) -> bool {
        if !self.contains(id) { return false; }
        self.bodies.interactions.grant(id);
        true
    }

    /// Removes a live static prop at the between-tick structural boundary.
    pub fn remove(&mut self, id: EntityId) -> bool {
        if !self.contains(id) { return false; }
        crate::remove_body(self.bodies, self.seekers, self.scenes, id, &mut self.trace).is_some()
    }
}

impl World {
    /// Restricted gameplay operations and lossless completed-tick outcomes.
    pub fn encounter_access(&mut self) -> (GameplayAccess<'_>, &[crate::Defeat]) {
        (GameplayAccess { bodies: &mut self.bodies, seekers: &mut self.seekers,
            scenes: &mut self.scenes, sources: &mut self.sources,
            trace: self.trace.sink(self.tick) }, &self.defeats)
    }
}
