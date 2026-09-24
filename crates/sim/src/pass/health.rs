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
use crate::{Bodies, EntityId, SourceId};

/// An authoritative defeat from the most recently completed simulation tick.
/// Kept independently of diagnostics, including the producer after body removal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Defeat {
    /// Retired generational body identity.
    pub body: EntityId,
    /// Final ground position before removal.
    pub position: glam::Vec2,
    /// Scene responsible for this body, retained for visual cleanup.
    pub scene: Option<crate::SceneId>,
    /// Source that actually produced this enemy, if any.
    pub source: Option<SourceId>,
}

/// Starting and maximum health for ordinary enemies, in health points.
pub const ENEMY_HIT_POINTS: u8 = 3;
const _: () = assert!(ENEMY_HIT_POINTS > 0, "an enemy must spawn alive");

#[derive(Default)]
pub(crate) struct Health {
    who: Members,
    remaining: Vec<u8>,
    origins: Vec<Option<SourceId>>,
}

/// A hit producer may subtract positive damage, but cannot grant health, heal, or
/// remove a body while it is iterating the world's dense body slices.
pub(crate) struct DamageSink<'a> {
    health: &'a mut Health,
}

impl DamageSink<'_> {
    /// Whether this identity is damageable; props and the player are excluded.
    pub(crate) fn contains(&self, id: EntityId) -> bool {
        self.health.who.index(id).is_some()
    }

    /// Subtracts positive damage, clamped at zero, from a damageable entity; absent membership is immune.
    pub(crate) fn hit(&mut self, id: EntityId, amount: core::num::NonZeroU8) -> Option<u8> {
        let row = self.health.who.index(id)?;
        let remaining = &mut self.health.remaining[row];
        assert!(*remaining > 0, "a defeated body cannot take another hit");
        *remaining = remaining.saturating_sub(amount.get());
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
            self.origins.push(None);
        }
    }

    pub(crate) fn set_origin(&mut self, id: EntityId, source: Option<SourceId>) {
        if let Some(row) = self.who.index(id) { self.origins[row] = source; }
    }

    pub(crate) fn ids(&self) -> &[EntityId] {
        self.who.ids()
    }

    pub(crate) fn revoke(&mut self, id: EntityId) {
        if let Some(row) = self.who.remove(id) {
            self.remaining.swap_remove(row);
            self.origins.swap_remove(row);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.remaining.len()
    }

    pub(crate) fn get(&self, id: EntityId) -> Option<u8> {
        self.who.index(id).map(|row| self.remaining[row])
    }

    pub(crate) fn hash(&self, hash: &mut Fnv) {
        let Self { who, remaining, origins } = self;
        who.hash(hash);
        for origin in origins {
            hash.usize(usize::from(origin.is_some()));
            if let Some(id) = origin { id.hash_into(hash); }
        }
        for value in remaining {
            hash.usize(usize::from(*value));
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.object("enemy_sources", |out| {
            for (&id, source) in self.who.ids().iter().zip(&self.origins) {
                if let Some(source) = source { out.text(&id.to_string(), &source.to_string()); }
            }
        });
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
    defeats: &mut Vec<Defeat>,
    mut trace: TraceSink<'_>,
) {
    defeats.clear();
    // Reverse order makes every `swap_remove` pull from a row already visited.
    // A survivor therefore needs no second visit, and no defeat queue exists.
    for row in (0..bodies.health.len()).rev() {
        let id = bodies.health.ids()[row];
        if bodies.health.get(id) != Some(0) {
            continue;
        }
        let source = bodies.health.origins[row];
        let removed = crate::remove_body(bodies, seekers, scenes, id, &mut trace)
            .expect("a defeated body must still be alive in the final pass");
        defeats.push(Defeat { body: id, source, position: removed.position, scene: removed.scene });
    }
}
