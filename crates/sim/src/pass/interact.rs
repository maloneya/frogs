//! One press activates the nearest ready member within reach.
//!
//! Runs after movement and collision resolution, before the final removal
//! boundary. It reads final positions and can change only interaction state.
//! Trace output observes the transition; future gameplay consumes the state.

use arpg_core::Report;
use glam::Vec2;
use serde::Deserialize;

use crate::EntityId;
use crate::hash::Fnv;
use crate::members::Members;
use crate::slots::Slots;
use crate::trace::{Event, TraceSink};

/// Centre-to-centre interaction reach in metres, inclusive at the boundary.
pub const INTERACTION_REACH: f32 = 1.5;
const _: () = assert!(INTERACTION_REACH.is_finite()
    && INTERACTION_REACH > crate::PLAYER_RADIUS + crate::ENEMY_RADIUS);

/// The one-shot state shared by gameplay, scenarios, and presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum InteractionState {
    /// Can be activated by an eligible nearby actor.
    Ready,
    /// Already used; further presses leave it unchanged.
    Activated,
}

impl core::fmt::Display for InteractionState {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        out.write_str(match self {
            Self::Ready => "Ready",
            Self::Activated => "Activated",
        })
    }
}

#[derive(Default)]
pub(crate) struct Interactions {
    who: Members,
    states: Vec<InteractionState>,
}

impl Interactions {
    pub(crate) fn grant(&mut self, id: EntityId) {
        if let Some(row) = self.who.add(id) {
            assert_eq!(row, self.states.len());
            self.states.push(InteractionState::Ready);
        }
    }

    pub(crate) fn revoke(&mut self, id: EntityId) {
        if let Some(row) = self.who.remove(id) {
            self.states.swap_remove(row);
        }
    }

    pub(crate) fn get(&self, id: EntityId) -> Option<InteractionState> {
        self.who.index(id).map(|row| self.states[row])
    }

    pub(crate) fn ids(&self) -> &[EntityId] { self.who.ids() }

    pub(crate) fn activate(&mut self, id: EntityId, trace: &mut TraceSink<'_>) -> bool {
        let Some(row) = self.who.index(id) else { return false; };
        if self.states[row] != InteractionState::Ready { return false; }
        self.states[row] = InteractionState::Activated;
        trace.emit(Event::Activated { id });
        true
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { who, states } = self;
        who.hash(h);
        for state in states {
            h.usize(*state as usize);
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.num("interaction_reach", INTERACTION_REACH);
        out.object("interactions", |out| {
            for (&id, &state) in self.who.ids().iter().zip(&self.states) {
                out.object(&id.to_string(), |out| {
                    out.bool("activated", state == InteractionState::Activated);
                });
            }
        });
    }
}

/// Shared admission for controlled and autonomous interaction producers.
pub(crate) fn can_activate(actor: EntityId, target: EntityId, interactions: &Interactions, slots: &Slots, positions: &[Vec2],
    health: &super::health::Health, walls: &crate::wall::Walls) -> bool {
    let (Some(from), Some(to)) = (slots.index(actor), slots.index(target)) else { return false; };
    interactions.get(target) == Some(InteractionState::Ready) && actor != target && (from == 0 || health.get(actor) != Some(0)) && health.get(target) != Some(0)
        && positions[from].distance_squared(positions[to]) <= INTERACTION_REACH * INTERACTION_REACH
        && !walls.obstructs(positions[from], positions[to])
}

#[expect(clippy::too_many_arguments, reason = "shared activation receives explicit identities and narrow physical, health and interaction capabilities")]
pub(crate) fn activate(interactions: &mut Interactions, slots: &Slots, positions: &[Vec2],
    health: &super::health::Health, walls: &crate::wall::Walls,
    actor: EntityId, target: EntityId, trace: &mut TraceSink<'_>) -> bool {
    can_activate(actor, target, interactions, slots, positions, health, walls) && interactions.activate(target, trace)
}

#[expect(clippy::too_many_arguments, reason = "nearest-endpoint selection reads narrow shared admission facts and writes only interactions")]
pub(crate) fn interact(
    interactions: &mut Interactions,
    slots: &Slots,
    positions: &[Vec2],
    actor: EntityId,
    pressed: bool,
    health: &super::health::Health,
    walls: &crate::wall::Walls,
    mut trace: TraceSink<'_>,
) {
    if !pressed {
        return;
    }
    let Some(actor_row) = slots.index(actor) else { return; };
    let position = positions[actor_row];
    // Walk only interaction members. Dense storage order may change on removal;
    // distance, then stable slot identity, determines the winner regardless.
    let mut nearest: Option<(f32, EntityId)> = None;
    for &id in interactions.who.ids() {
        let Some(body) = slots.index(id) else {
            continue;
        };
        if !can_activate(actor, id, interactions, slots, positions, health, walls) { continue; }
        let distance = position.distance_squared(positions[body]);
        if nearest.is_none_or(|(best, other)| {
            distance < best || (distance == best && id.slot() < other.slot())
        }) {
            nearest = Some((distance, id));
        }
    }
    if let Some((_, id)) = nearest {
        activate(interactions, slots, positions, health, walls, actor, id, &mut trace);
    }
}
