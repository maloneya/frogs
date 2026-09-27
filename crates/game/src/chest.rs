//! Scene-owned chests. Unlocking grants interaction; opening keeps the physical
//! prop and exposes a persistent open state to presentation.
use crate::encounter::EncounterEvent;
use arpg_core::Report;
use arpg_sim::{EntityId, Fnv, GameplayAccess, InteractionState, SceneId, TraceSink};
use serde::{Deserialize, Serialize};

/// A locked chest bound to an explicit, non-interactable static scene body.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChestSpec {
    /// Index in the engine scene's explicit bodies.
    pub body: usize,
}

/// Authoritative chest state, independent of whether its prop still exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ChestState {
    /// Blocks movement and cannot consume interaction.
    Locked,
    /// Still blocks movement; a fresh nearby interaction opens it.
    Unlocked,
    /// The lid is open; the physical prop remains in the scene.
    Open,
    /// The prop disappeared through another lifetime operation.
    Missing,
}

struct Chest {
    owner: SceneId,
    body: EntityId,
    state: ChestState,
}

#[derive(Default)]
pub(crate) struct Chests {
    rows: Vec<Chest>,
}

/// Admission bound also limits the extra chest geometry in presentation.
pub const MAX_CHESTS: usize = 32;

impl Chests {
    pub(crate) fn for_body(&self, body: EntityId) -> Option<ChestState> {
        self.rows.iter().find(|row| row.body == body).map(|row| row.state)
    }
    pub(crate) fn validate(
        &self,
        specs: &[ChestSpec],
        bodies: &[arpg_sim::Placed],
        doors: &[crate::DoorSpec],
    ) -> Result<(), crate::SceneError> {
        if specs.len() > MAX_CHESTS - self.rows.len() {
            return Err(crate::SceneError::Encounter("too many live chests"));
        }
        for (nth, chest) in specs.iter().enumerate() {
            if doors.iter().any(|door| door.body == chest.body) {
                return Err(crate::SceneError::Encounter("a body cannot be both a chest and a door"));
            }
            if !bodies
                .get(chest.body)
                .is_some_and(|body| body.what.fixed() && !body.what.interactable())
            {
                return Err(crate::SceneError::Encounter(
                    "chest must bind an explicit non-interactable static prop",
                ));
            }
            if specs[..nth].iter().any(|other| other.body == chest.body) {
                return Err(crate::SceneError::Encounter("duplicate chest body"));
            }
        }
        Ok(())
    }
    pub(crate) fn install(&mut self, owner: SceneId, specs: &[ChestSpec], bodies: &[EntityId]) {
        self.rows.extend(specs.iter().map(|spec| Chest {
            owner,
            body: bodies[spec.body],
            state: ChestState::Locked,
        }));
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|row| row.owner != owner);
    }
    fn row(&self, owner: SceneId, nth: usize) -> Option<&Chest> {
        self.rows.iter().filter(|row| row.owner == owner).nth(nth)
    }
    pub(crate) fn state(&self, owner: SceneId, nth: usize) -> Option<ChestState> {
        self.row(owner, nth).map(|row| row.state)
    }
    pub(crate) fn available(
        &self,
        owner: SceneId,
        nth: usize,
        access: &GameplayAccess<'_>,
    ) -> bool {
        self.row(owner, nth)
            .is_some_and(|row| access.contains(row.body))
    }
    pub(crate) fn unlock(
        &mut self,
        owner: SceneId,
        nth: usize,
        access: &mut GameplayAccess<'_>,
        trace: &mut TraceSink<'_, EncounterEvent>,
    ) {
        let Some(row) = self
            .rows
            .iter_mut()
            .filter(|row| row.owner == owner)
            .nth(nth)
        else {
            return;
        };
        if row.state == ChestState::Locked && access.enable_interaction(row.body) {
            row.state = ChestState::Unlocked;
            trace.emit(EncounterEvent::Chest {
                scene: owner,
                index: nth,
                state: row.state,
            });
        }
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for row in &self.rows {
            row.owner.hash_into(out);
            row.body.hash_into(out);
            out.usize(row.state as usize);
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("chests", |out| {
            for group in self.rows.chunk_by(|a, b| a.owner == b.owner) {
                for (nth, row) in group.iter().enumerate() {
                    out.object(&format!("{}/{nth}", row.owner), |out| {
                        out.text("body", &row.body.to_string());
                        out.text("state", &format!("{:?}", row.state));
                    });
                }
            }
        });
    }
}

/// Before simulation: consume last tick's chest interaction and retain its prop.
/// A missing body becomes Missing, never Open, and stale IDs cannot act on replacements.
pub(crate) fn advance(
    chests: &mut Chests,
    access: &GameplayAccess<'_>,
    trace: &mut TraceSink<'_, EncounterEvent>,
) {
    for group in chests.rows.chunk_by_mut(|a, b| a.owner == b.owner) {
        for (nth, row) in group.iter_mut().enumerate() {
            let next = match row.state {
                ChestState::Missing => continue,
                ChestState::Locked | ChestState::Unlocked | ChestState::Open
                    if !access.contains(row.body) => ChestState::Missing,
                ChestState::Unlocked
                    if access.interaction(row.body) == Some(InteractionState::Activated) =>
                {
                    ChestState::Open
                }
                ChestState::Locked | ChestState::Unlocked | ChestState::Open => continue,
            };
            row.state = next;
            trace.emit(EncounterEvent::Chest {
                scene: row.owner,
                index: nth,
                state: next,
            });
        }
    }
}
