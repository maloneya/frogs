//! Gameplay bindings to physical panels. Latch state belongs exclusively to sim.
use crate::encounter::EncounterEvent;
use arpg_core::Report;
use arpg_sim::{EntityId, Fnv, GameplayAccess, LatchState, PanelConfig, SceneId, TraceSink, World};
use serde::{Deserialize, Serialize};

/// A hinged panel attached to an explicit neutral body in this scene.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DoorSpec {
    /// Index in the scene's explicit bodies.
    pub body: usize,
    /// Validated physical shape, angular limits and latch strength.
    pub panel: PanelConfig,
}

/// Observation of a door's physical attachment, never a second simulation state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum DoorState {
    /// The latch still restrains the panel.
    Latched,
    /// An explicit mechanism released the latch without breaking it.
    Released,
    /// Physical reaction exceeded the latch strength.
    Broken,
    /// The physical body was removed; distinct from a legitimate breach.
    Missing,
}
impl From<LatchState> for DoorState {
    fn from(state: LatchState) -> Self {
        match state {
            LatchState::Latched => Self::Latched,
            LatchState::Released => Self::Released,
            LatchState::Broken => Self::Broken,
        }
    }
}

struct Door { owner: SceneId, body: EntityId }

#[derive(Default)]
pub(crate) struct Doors { rows: Vec<Door> }

impl Doors {
    pub(crate) fn accepts(actor: &crate::ActorDefinition) -> bool {
        !actor.physical.fixed() && !actor.physical.is_character()
            && actor.role == crate::ActorRole::Prop && actor.actions.is_empty()
    }
    /// Gameplay's rule only; simulation admission owns the body binding,
    /// uniqueness, capacity and swing checks for the panel itself.
    pub(crate) fn validate(specs: &[DoorSpec], bodies: &[crate::Placed]) -> Result<(), crate::SceneError> {
        if specs.iter().any(|door| bodies.get(door.body).is_some_and(|body| !Self::accepts(&body.what))) {
            return Err(crate::SceneError::Encounter("door must bind a movable prop without actor actions"));
        }
        Ok(())
    }
    pub(crate) fn install(&mut self, owner: SceneId, specs: &[DoorSpec], bodies: &[EntityId]) {
        self.rows.extend(specs.iter().map(|spec| Door { owner, body: bodies[spec.body] }));
    }
    pub(crate) fn evict(&mut self, owner: SceneId) { self.rows.retain(|row| row.owner != owner); }
    fn row(&self, owner: SceneId, nth: usize) -> Option<&Door> {
        self.rows.iter().filter(|row| row.owner == owner).nth(nth)
    }
    pub(crate) fn state(&self, owner: SceneId, nth: usize, world: &World) -> Option<DoorState> {
        self.row(owner, nth).map(|row| world.panel(row.body)
            .map_or(DoorState::Missing, |panel| panel.latch().into()))
    }
    pub(crate) fn available(&self, owner: SceneId, nth: usize, access: &GameplayAccess<'_>) -> bool {
        self.row(owner, nth).is_some_and(|row| access.panel(row.body).is_some())
    }
    pub(crate) fn unlock(&self, owner: SceneId, nth: usize, access: &mut GameplayAccess<'_>, trace: &mut TraceSink<'_, EncounterEvent>) {
        let Some(row) = self.row(owner, nth) else { return };
        if access.release_latch(row.body) {
            trace.emit(EncounterEvent::Door { scene: owner, index: nth, state: DoorState::Released });
        }
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for row in &self.rows { row.owner.hash_into(out); row.body.hash_into(out); }
    }
    pub(crate) fn report(&self, world: &World, out: &mut Report) {
        out.object("doors", |out| {
            for group in self.rows.chunk_by(|a, b| a.owner == b.owner) {
                for (nth, row) in group.iter().enumerate() {
                    out.object(&format!("{}/{nth}", row.owner), |out| {
                        out.text("body", &row.body.to_string());
                        out.text("state", &format!("{:?}", self.state(row.owner, nth, world).expect("resident door")));
                    });
                }
            }
        });
    }
}
