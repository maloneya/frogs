//! Damageable membership and the final structural boundary for defeat.
//!
//! Health is sparse now that static props share body storage with enemies.
//! A stable id, rather than a body row, is the only door into damage. Props
//! have no health membership, so a hit cannot accidentally damage one after
//! a dense-row swap. Only the final pass may remove defeated bodies.

use arpg_core::Report;

use crate::hash::Fnv;
use crate::members::Members;
use crate::scene::Scenes;
use crate::trace::TraceSink;
use crate::{Bodies, EntityId};

const ENEMY_HIT_POINTS: u8 = 3;
const _: () = assert!(ENEMY_HIT_POINTS > 0, "an enemy must survive at least one hit");

#[derive(Default)]
pub(crate) struct Health {
    who: Members,
    remaining: Vec<u8>,
}

/// A hit producer may subtract one point, but cannot grant health, heal, or
/// remove a body while it is iterating the world's dense body slices.
pub(crate) struct DamageSink<'a> {
    health: &'a mut Health,
}

impl DamageSink<'_> {
    /// Subtracts one hit from a damageable entity; absent membership is immune.
    pub(crate) fn hit(&mut self, id: EntityId) -> Option<u8> {
        let row = self.health.who.index(id)?;
        let remaining = &mut self.health.remaining[row];
        *remaining = remaining.checked_sub(1).expect("a defeated body cannot take another hit");
        Some(*remaining)
    }
}

impl Health {
    pub(crate) fn sink(&mut self) -> DamageSink<'_> {
        DamageSink { health: self }
    }

    pub(crate) fn grant(&mut self, id: EntityId) {
        if let Some(row) = self.who.add(id) {
            assert_eq!(row, self.remaining.len());
            self.remaining.push(ENEMY_HIT_POINTS);
        }
    }

    pub(crate) fn ids(&self) -> &[EntityId] {
        self.who.ids()
    }

    pub(crate) fn revoke(&mut self, id: EntityId) {
        if let Some(row) = self.who.remove(id) {
            self.remaining.swap_remove(row);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.remaining.len()
    }

    pub(crate) fn get(&self, id: EntityId) -> Option<u8> {
        self.who.index(id).map(|row| self.remaining[row])
    }

    pub(crate) fn hash(&self, hash: &mut Fnv) {
        let Self { who, remaining } = self;
        who.hash(hash);
        for value in remaining {
            hash.usize(usize::from(*value));
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.object("health", |out| {
            for (&id, &value) in self.who.ids().iter().zip(&self.remaining) {
                out.int(&id.to_string(), u64::from(value));
            }
        });
    }
}

/// Removes bodies whose last hit reduced them to zero. This runs last because
/// removal swap-moves the horde's parallel arrays.
pub(crate) fn remove_defeated(
    bodies: &mut Bodies,
    seekers: &mut Members,
    scenes: &mut Scenes,
    mut trace: TraceSink<'_>,
) {
    // Reverse order makes every `swap_remove` pull from a row already visited.
    // A survivor therefore needs no second visit, and no defeat queue exists.
    for row in (0..bodies.health.len()).rev() {
        let id = bodies.health.ids()[row];
        if bodies.health.get(id) != Some(0) {
            continue;
        }
        let removed = crate::remove_body(bodies, seekers, scenes, id, &mut trace);
        debug_assert!(removed, "a defeated body must still be alive in the final pass");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_hits_reach_zero() {
        let mut health = Health::default();
        let id = crate::slots::Slots::default().insert();
        health.grant(id);

        assert_eq!(health.sink().hit(id), Some(2));
        assert_eq!(health.sink().hit(id), Some(1));
        assert_eq!(health.sink().hit(id), Some(0));
    }
}
