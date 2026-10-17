//! Damageable membership and the final structural boundary for defeat.
//!
//! Health is an explicit capability independent of movement and presentation.
//! Effects resolve stable identities, so row swaps cannot retarget damage.
//! Only the final pass removes defeated nonplayer bodies; the protected player
//! remains allocated at zero health.

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
    /// Defeated generational body identity; physics may still be settling.
    pub body: EntityId,
    /// Ground position on the lethal tick.
    pub position: glam::Vec2,
    /// Scene responsible for this body, retained for visual cleanup.
    pub scene: Option<crate::SceneId>,
    /// Source that actually produced this enemy, if any.
    pub source: Option<SourceId>,
}

#[derive(Default)]
pub(crate) struct Health {
    who: Members,
    remaining: Vec<u8>,
    maximum: Vec<u8>,
    origins: Vec<Option<SourceId>>,
    announced: Vec<bool>,
}

impl Health {
    /// Subtract positive damage once from a live health member; immunity and defeat refuse it.
    pub(crate) fn hit(&mut self, id: EntityId, amount: core::num::NonZeroU8) -> Option<u8> {
        let row = self.who.index(id)?;
        let remaining = &mut self.remaining[row];
        if *remaining == 0 { return None; }
        *remaining = remaining.saturating_sub(amount.get());
        Some(*remaining)
    }

    pub(crate) fn grant(&mut self, id: EntityId, initial: core::num::NonZeroU8) {
        if let Some(row) = self.who.add(id) {
            assert_eq!(row, self.remaining.len());
            self.remaining.push(initial.get());
            self.maximum.push(initial.get());
            self.origins.push(None);
            self.announced.push(false);
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
            self.maximum.swap_remove(row);
            self.origins.swap_remove(row);
            self.announced.swap_remove(row);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.remaining.len()
    }

    pub(crate) fn get(&self, id: EntityId) -> Option<u8> {
        self.who.index(id).map(|row| self.remaining[row])
    }

    pub(crate) fn maximum(&self, id: EntityId) -> Option<u8> {
        self.who.index(id).map(|row| self.maximum[row])
    }

    pub(crate) fn hash(&self, hash: &mut Fnv) {
        let Self { who, remaining, maximum, origins, announced } = self;
        who.hash(hash);
        for value in announced { hash.usize(usize::from(*value)); }
        for origin in origins {
            hash.usize(usize::from(origin.is_some()));
            if let Some(id) = origin { id.hash_into(hash); }
        }
        for value in maximum { hash.usize(usize::from(*value)); }
        for value in remaining {
            hash.usize(usize::from(*value));
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.object("defeat_announced", |out| {
            for (&id, &value) in self.who.ids().iter().zip(&self.announced) {
                out.bool(&id.to_string(), value);
            }
        });
        out.object("body_sources", |out| {
            for (&id, source) in self.who.ids().iter().zip(&self.origins) {
                if let Some(source) = source { out.text(&id.to_string(), &source.to_string()); }
            }
        });
        out.object("max_health", |out| {
            for (&id, &value) in self.who.ids().iter().zip(&self.maximum) { out.int(&id.to_string(), u64::from(value)); }
        });
        out.object("health", |out| {
            for (&id, &value) in self.who.ids().iter().zip(&self.remaining) {
                out.int(&id.to_string(), u64::from(value));
            }
        });
    }
}

/// Publishes death once, retaining character physics until motion rests. Runs
/// last because removal swap-moves the shared body arrays.
pub(crate) fn remove_defeated(
    bodies: &mut Bodies,
    scenes: &mut Scenes,
    defeats: &mut Vec<Defeat>,
    mut trace: TraceSink<'_>,
) {
    defeats.clear();
    // Reverse order makes every `swap_remove` pull from a row already visited.
    // A survivor therefore needs no second visit, and no defeat queue exists.
    for row in (0..bodies.health.len()).rev() {
        let id = bodies.health.ids()[row];
        if id == bodies.slots.ids()[0] || bodies.health.get(id) != Some(0) {
            continue;
        }
        let source = bodies.health.origins[row];
        let newly_defeated = !bodies.health.announced[row];
        if newly_defeated {
            bodies.health.announced[row] = true;
            defeats.push(Defeat {
                body: id, source, position: bodies.pos_of(id).expect("health owns a body"),
                scene: scenes.body_owner(id),
            });
        }
        let character = bodies.characters.contains(id);
        // Attack impulses arrive after the solver. Retain the body until the
        // shared motion owner snaps it to rest, preserving all carried momentum.
        if character
            && bodies.physics.get(id).is_some_and(|motion| motion.velocity() != glam::Vec2::ZERO)
        {
            continue;
        }
        crate::remove_body(bodies, scenes, id, &mut trace)
            .expect("a defeated body must still be alive in the final pass");
    }
}
