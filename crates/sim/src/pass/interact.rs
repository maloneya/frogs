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
    /// Can be activated by a nearby player.
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

pub(crate) fn interact(
    interactions: &mut Interactions,
    slots: &Slots,
    positions: &[Vec2],
    player: Vec2,
    pressed: bool,
    mut trace: TraceSink<'_>,
) {
    if !pressed {
        return;
    }
    // Walk only interaction members. Dense storage order may change on removal;
    // distance, then stable slot identity, determines the winner regardless.
    let mut nearest: Option<(f32, EntityId, usize)> = None;
    for (row, &id) in interactions.who.ids().iter().enumerate() {
        if interactions.states[row] != InteractionState::Ready {
            continue;
        }
        let Some(body) = slots.index(id) else {
            continue;
        };
        let distance = player.distance_squared(positions[body]);
        if distance > INTERACTION_REACH * INTERACTION_REACH {
            continue;
        }
        if nearest.is_none_or(|(best, other, _)| {
            distance < best || (distance == best && id.slot() < other.slot())
        }) {
            nearest = Some((distance, id, row));
        }
    }
    if let Some((_, id, row)) = nearest {
        interactions.states[row] = InteractionState::Activated;
        trace.emit(Event::Activated { id });
    }
}
