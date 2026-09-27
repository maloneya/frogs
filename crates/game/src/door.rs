//! Door lifetime and interaction. Closed doors are ordinary static props;
//! unlocking grants interaction, opening retires the prop at a structural boundary.
use crate::encounter::EncounterEvent;
use arpg_core::Report;
use arpg_sim::{EntityId, Fnv, GameplayAccess, InteractionState, SceneId, TraceSink};
use serde::{Deserialize, Serialize};

/// A locked door bound to an explicit, non-interactable static scene body.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DoorSpec {
    /// Index in the engine scene's explicit bodies.
    pub body: usize,
}

/// Authoritative door state, independent of whether its prop still exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum DoorState {
    /// Blocks movement and cannot consume interaction.
    Locked,
    /// Still blocks movement; a fresh nearby interaction opens it.
    Unlocked,
    /// The blocking prop has been removed by the door system.
    Open,
    /// The closed prop disappeared through another lifetime operation.
    Missing,
}

struct Door {
    owner: SceneId,
    body: EntityId,
    state: DoorState,
}

#[derive(Default)]
pub(crate) struct Doors {
    rows: Vec<Door>,
}

impl Doors {
    pub(crate) fn validate(
        specs: &[DoorSpec],
        bodies: &[crate::Placed],
    ) -> Result<(), crate::SceneError> {
        for (nth, door) in specs.iter().enumerate() {
            if !bodies
                .get(door.body)
                .is_some_and(|body| body.what.physical.fixed() && !body.what.physical.is_character() && !body.what.physical.interactable())
            {
                return Err(crate::SceneError::Encounter(
                    "door must bind an explicit non-interactable static prop",
                ));
            }
            if specs[..nth].iter().any(|other| other.body == door.body) {
                return Err(crate::SceneError::Encounter("duplicate door body"));
            }
        }
        Ok(())
    }
    pub(crate) fn install(&mut self, owner: SceneId, specs: &[DoorSpec], bodies: &[EntityId]) {
        self.rows.extend(specs.iter().map(|spec| Door {
            owner,
            body: bodies[spec.body],
            state: DoorState::Locked,
        }));
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|row| row.owner != owner);
    }
    fn row(&self, owner: SceneId, nth: usize) -> Option<&Door> {
        self.rows.iter().filter(|row| row.owner == owner).nth(nth)
    }
    pub(crate) fn state(&self, owner: SceneId, nth: usize) -> Option<DoorState> {
        self.row(owner, nth).map(|row| row.state)
    }
    pub(crate) fn available(
        &self,
        owner: SceneId,
        nth: usize,
        access: &GameplayAccess<'_>,
    ) -> bool {
        self.row(owner, nth)
            .is_some_and(|row| row.state == DoorState::Open || access.contains(row.body))
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
        if row.state == DoorState::Locked && access.enable_interaction(row.body) {
            row.state = DoorState::Unlocked;
            trace.emit(EncounterEvent::Door {
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
        out.object("doors", |out| {
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

/// Before simulation: consume last tick's door interaction and remove its prop.
/// A missing closed body becomes Missing, never Open, and stale IDs cannot act on replacements.
pub(crate) fn advance(
    doors: &mut Doors,
    access: &mut GameplayAccess<'_>,
    trace: &mut TraceSink<'_, EncounterEvent>,
) {
    for group in doors.rows.chunk_by_mut(|a, b| a.owner == b.owner) {
        for (nth, row) in group.iter_mut().enumerate() {
            let next = match row.state {
                DoorState::Open | DoorState::Missing => continue,
                DoorState::Locked | DoorState::Unlocked
                    if !access.contains(row.body) => DoorState::Missing,
                DoorState::Unlocked
                    if access.interaction(row.body) == Some(InteractionState::Activated) =>
                {
                    assert!(access.remove(row.body));
                    DoorState::Open
                }
                DoorState::Locked | DoorState::Unlocked => continue,
            };
            row.state = next;
            trace.emit(EncounterEvent::Door {
                scene: row.owner,
                index: nth,
                state: next,
            });
        }
    }
}
