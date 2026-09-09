//! Three-hit enemy durability and the structural boundary for defeat.
//!
//! Health rows are parallel to the horde rows in [`crate::Bodies`]. An attack
//! may subtract through [`DamageSink`], but the sink cannot touch body storage.
//! Zero-health rows wait until [`remove_defeated`] runs last, after every pass
//! that reads dense body rows has finished.

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
    remaining: Vec<u8>,
}

/// A hit producer may subtract one point, but cannot grant health, heal, or
/// remove a body while it is iterating the world's dense body slices.
pub(crate) struct DamageSink<'a> {
    health: &'a mut Health,
}

impl DamageSink<'_> {
    /// Damages the enemy at this horde row and returns its new health.
    pub(crate) fn hit(&mut self, row: usize) -> u8 {
        let remaining = &mut self.health.remaining[row];
        *remaining = remaining.checked_sub(1).expect("a defeated body cannot take another hit");
        *remaining
    }
}

impl Health {
    pub(crate) fn sink(&mut self) -> DamageSink<'_> {
        DamageSink { health: self }
    }

    pub(crate) fn grant(&mut self) {
        self.remaining.push(ENEMY_HIT_POINTS);
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        self.remaining.reserve(additional);
    }

    pub(crate) fn revoke(&mut self, row: usize) {
        self.remaining.swap_remove(row);
    }

    pub(crate) fn clear(&mut self) {
        self.remaining.clear();
    }

    pub(crate) fn len(&self) -> usize {
        self.remaining.len()
    }

    pub(crate) fn get(&self, row: usize) -> u8 {
        self.remaining[row]
    }

    pub(crate) fn hash(&self, hash: &mut Fnv) {
        hash.usize(self.remaining.len());
        for value in &self.remaining {
            hash.usize(usize::from(*value));
        }
    }

    pub(crate) fn report(&self, ids: &[EntityId], out: &mut Report) {
        debug_assert_eq!(ids.len(), self.remaining.len());
        out.object("health", |out| {
            for (&id, &value) in ids.iter().zip(&self.remaining) {
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
        if bodies.health.get(row) != 0 {
            continue;
        }
        let id = bodies.slots.ids()[row + 1];
        let removed = crate::remove_enemy(bodies, seekers, scenes, id, &mut trace);
        debug_assert!(removed, "a defeated body must still be alive in the final pass");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_hits_reach_zero() {
        let mut health = Health::default();
        health.grant();

        assert_eq!(health.sink().hit(0), 2);
        assert_eq!(health.sink().hit(0), 1);
        assert_eq!(health.sink().hit(0), 0);
    }
}
