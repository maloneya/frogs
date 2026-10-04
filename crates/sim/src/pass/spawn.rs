//! Queued body creation, drained before passes borrow dense rows.
//!
//! Requests are data so they can be serialized, hashed and replayed. They carry
//! optional producer identity for attribution; producers do not own storage.
//! Requests submitted before the drain can land that tick; later ones wait.

use glam::Vec2;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::MAX_BODIES;
use crate::hash::Fnv;
use crate::scene::Scenes;
use crate::slots::EntityId;
use crate::trace::{Event, TraceSink};
use crate::{Bodies, SceneId};

/// Pending-request bound. Cancelled requests retain slots until the next drain
/// so outstanding outcomes remain bounded too.
pub(crate) const QUEUE_CAPACITY: usize = 256;
const _: () = assert!(
    QUEUE_CAPACITY > 0,
    "a zero-length queue would refuse everything, silently"
);

/// Independent physical, health and presentation capabilities of a body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Template {
    fixed: bool,
    mass: crate::Mass,
    interactable: bool,
    health: Option<core::num::NonZeroU8>,
    character: bool,
}

impl Template {
    /// Neutral movable body, without health, interaction or character rendering.
    pub const BODY: Self = Self {
        fixed: false,
        mass: crate::Mass::UNIT,
        interactable: false,
        health: None,
        character: false,
    };
    /// Immovable neutral prop using the shared body footprint.
    pub const BLOCK: Self = Self {
        fixed: true,
        ..Self::BODY
    };
    /// Sets dynamic mass without coupling physics to health, rendering or allegiance.
    #[must_use]
    pub const fn with_mass(self, mass: crate::Mass) -> Self {
        Self { mass, ..self }
    }
    /// Authored mass in kilograms; fixed bodies have zero inverse mass regardless.
    #[must_use]
    pub const fn mass(self) -> crate::Mass {
        self.mass
    }
    /// Grants independently validated starting health.
    #[must_use]
    pub const fn with_health(self, health: core::num::NonZeroU8) -> Self {
        Self {
            health: Some(health),
            ..self
        }
    }
    /// Selects character presentation without implying hostility or health.
    #[must_use]
    pub const fn character(self) -> Self {
        Self {
            character: true,
            ..self
        }
    }
    /// Grants one-shot interaction independently of other capabilities.
    #[must_use]
    pub const fn interactive(self) -> Self {
        Self {
            interactable: true,
            ..self
        }
    }
    /// Initial health; absent means damage cannot affect this body.
    #[must_use]
    pub const fn hit_points(self) -> Option<core::num::NonZeroU8> {
        self.health
    }
    /// Whether this body uses character presentation.
    #[must_use]
    pub const fn is_character(self) -> bool {
        self.character
    }
    /// Whether this body supports interaction.
    #[must_use]
    pub const fn interactable(self) -> bool {
        self.interactable
    }
    /// Whether this body is immovable.
    #[must_use]
    pub const fn fixed(self) -> bool {
        self.fixed
    }
    /// Shared file and runtime admission. Static bodies must already fit the arena.
    pub(crate) fn valid_at(self, at: Vec2) -> bool {
        crate::valid_position(at)
            && (!self.fixed || at.abs().max_element() <= crate::ARENA_HALF - crate::ENEMY_RADIUS)
    }
    /// Hashes every admitted capability, including initial health and presentation.
    pub fn hash_into(self, h: &mut Fnv) {
        let Self {
            fixed,
            mass,
            interactable,
            health,
            character,
        } = self;
        h.f32(mass.kilograms());
        h.usize(usize::from(interactable));
        h.usize(usize::from(fixed));
        h.usize(health.map_or(0, |value| usize::from(value.get())));
        h.usize(usize::from(character));
    }
}

/// Stable identity for an accepted queued spawn, scoped to this world lifetime.
/// Scene eviction never reuses tickets; restarting creates a fresh lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnTicket(u64);

impl SpawnTicket {
    /// Stable numeric ticket identity for replay state owned by callers.
    pub fn hash_into(self, hash: &mut Fnv) {
        hash.u64(self.0);
    }
}

impl core::fmt::Display for SpawnTicket {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(out, "{}", self.0)
    }
}

/// Authoritative result of one accepted request at the latest spawn boundary.
/// `entity` is absent when admission fails or its owning scene was evicted.
/// Results remain valid observations even if the body is removed later that tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnOutcome {
    /// Ticket returned when the request was accepted.
    pub ticket: SpawnTicket,
    /// Body created, or refusal. Never inferred from diagnostic retention.
    pub entity: Option<EntityId>,
    /// Scene that owns the request, if any.
    pub owner: Option<SceneId>,
    /// Source that produced the request, if any.
    pub source: Option<crate::SourceId>,
}

/// One thing asked for, not yet made.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Request {
    ticket: SpawnTicket,
    cancelled: bool,
    /// Ground-plane position: `x` is world X and `y` is world **Z**.
    at: Vec2,
    what: Template,
    owner: Option<SceneId>,
    source: Option<crate::SourceId>,
}

/// What has been asked for and not yet granted.
///
/// FIFO, and drained in the order things were asked for. Not sorted, not
/// deduplicated, not batched by template: any of those would make the order in
/// which bodies take slots depend on something other than the order of the
/// requests, and slot order is what every id in a replay is keyed to.
pub(crate) struct SpawnQueue {
    pending: Vec<Request>,
    next_ticket: u64,
    /// Bounded by accepted requests; replaced at the next drain.
    outcomes: Vec<SpawnOutcome>,
    /// Requests dropped since the last drain, because the queue was full.
    ///
    /// Counted rather than ignored, because a dropped spawn is otherwise the
    /// perfect silent failure: nothing appears, nothing errors, and the only
    /// evidence is a body that a designer expected and did not get. The drain
    /// reports it and resets it.
    refused: usize,
}

impl Default for SpawnQueue {
    fn default() -> Self {
        // Allocated once, up front — see [`QUEUE_CAPACITY`].
        Self {
            pending: Vec::with_capacity(QUEUE_CAPACITY),
            next_ticket: 0,
            outcomes: Vec::with_capacity(QUEUE_CAPACITY),
            refused: 0,
        }
    }
}

impl SpawnQueue {
    /// Asks for one thing. `None` when the queue is full, in which case the
    /// request is dropped and counted.
    ///
    /// Refusing rather than growing keeps the allocation promise, and refusing
    /// the *newest* rather than the oldest keeps a flood from starving requests
    /// that were already accepted.
    pub(crate) fn push(&mut self, at: Vec2, what: Template) -> Option<SpawnTicket> {
        self.push_owned(at, what, None)
    }

    pub(crate) fn push_owned(
        &mut self,
        at: Vec2,
        what: Template,
        owner: Option<SceneId>,
    ) -> Option<SpawnTicket> {
        self.push_from(at, what, owner, None)
    }

    pub(crate) fn push_from(
        &mut self,
        at: Vec2,
        what: Template,
        owner: Option<SceneId>,
        source: Option<crate::SourceId>,
    ) -> Option<SpawnTicket> {
        if !what.valid_at(at) {
            return None;
        }
        if self.pending.len() >= QUEUE_CAPACITY {
            self.refused += 1;
            return None;
        }
        let ticket = SpawnTicket(self.next_ticket);
        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .expect("spawn ticket space exhausted");
        self.pending.push(Request {
            ticket,
            cancelled: false,
            at,
            what,
            owner,
            source,
        });
        Some(ticket)
    }

    /// How many requests are waiting. Reported in `state`, because "the spawn I
    /// asked for has not happened yet" and "the spawn I asked for was refused"
    /// look identical from the outside otherwise.
    pub(crate) fn len(&self) -> usize {
        self.pending
            .iter()
            .filter(|request| !request.cancelled)
            .count()
    }

    pub(crate) fn outcomes(&self) -> &[SpawnOutcome] {
        &self.outcomes
    }

    pub(crate) fn cancel_scene(&mut self, owner: SceneId) {
        // Keep the bounded request slot until the boundary so every accepted
        // ticket receives a result, even after scene eviction.
        for request in &mut self.pending {
            if request.owner == Some(owner) {
                request.cancelled = true;
            }
        }
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self {
            pending,
            next_ticket,
            outcomes,
            refused,
        } = self;
        h.u64(*next_ticket);
        h.usize(outcomes.len());
        for outcome in outcomes {
            let SpawnOutcome {
                ticket,
                entity,
                owner,
                source,
            } = outcome;
            h.u64(ticket.0);
            h.usize(usize::from(entity.is_some()));
            if let Some(id) = entity {
                id.hash_into(h);
            }
            h.usize(usize::from(owner.is_some()));
            if let Some(id) = owner {
                id.hash_into(h);
            }
            h.usize(usize::from(source.is_some()));
            if let Some(id) = source {
                id.hash_into(h);
            }
        }

        h.usize(pending.len());
        h.usize(*refused);
        for request in pending {
            let Request {
                ticket,
                cancelled,
                at,
                what,
                owner,
                source,
            } = request;
            h.u64(ticket.0);
            h.usize(usize::from(*cancelled));
            h.f32(at.x);
            h.f32(at.y);
            what.hash_into(h);
            h.usize(usize::from(source.is_some()));
            if let Some(source) = source {
                source.hash_into(h);
            }
            h.usize(usize::from(owner.is_some()));
            if let Some(owner) = owner {
                owner.hash_into(h);
            }
        }
    }
}

/// Shared immediate/queued placement. Returns `None` at capacity.
/// `Bodies` owns parallel-array updates and seeds interpolation endpoints.
pub(crate) fn place(
    enemies: &mut Bodies,
    at: Vec2,
    what: Template,
    trace: &mut TraceSink<'_>,
) -> Option<EntityId> {
    let id = enemies.spawn(at, what)?;

    // Diagnostics describe placement; authoritative outcomes bind request identities.
    trace.emit(Event::Placed { id });

    Some(id)
}

/// Drains queued requests before row readers, admitting bodies for the full tick.
pub(crate) fn drain(
    queue: &mut SpawnQueue,
    enemies: &mut Bodies,
    scenes: &mut Scenes,
    mut trace: TraceSink<'_>,
) {
    // Requests refused at the door plus requests refused here are one number to
    // whoever asked: in both cases they asked and did not get it.
    let mut refused = core::mem::take(&mut queue.refused);

    // `drain` rather than iterate-then-clear: the queue must be empty
    // afterwards even if the horde is full, or a request refused this tick
    // would be retried on every tick forever.
    queue.outcomes.clear();
    for request in queue.pending.drain(..) {
        let entity = if request.cancelled || request.owner.is_some_and(|id| !scenes.contains(id)) {
            None
        } else {
            place(enemies, request.at, request.what, &mut trace)
        };
        if let Some(id) = entity {
            enemies.health.set_origin(id, request.source);
            if let Some(owner) = request.owner {
                scenes.record_body(owner, id);
            }
        } else {
            refused += 1;
        }
        queue.outcomes.push(SpawnOutcome {
            ticket: request.ticket,
            entity,
            owner: request.owner,
            source: request.source,
        });
    }

    if refused > 0 {
        trace.emit(Event::Refused { count: refused });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bodies;

    /// Exercise capacity refusal without paying for an unrelated full-world step.
    #[test]
    fn a_full_horde_refuses_what_it_cannot_hold() {
        let mut enemies = Bodies::default();
        for _ in 0..MAX_BODIES {
            enemies.spawn(Vec2::ZERO, crate::TEST_CHARACTER).unwrap();
        }
        assert_eq!(
            enemies.len(),
            MAX_BODIES + 1,
            "the horde did not actually fill"
        );

        let mut queue = SpawnQueue::default();
        let mut trace = crate::Trace::default();

        assert!(
            queue.push(Vec2::ZERO, crate::TEST_CHARACTER).is_some(),
            "the queue refused before the horde could"
        );
        drain(
            &mut queue,
            &mut enemies,
            &mut Scenes::default(),
            trace.sink(0),
        );

        assert_eq!(enemies.len(), MAX_BODIES + 1, "the budget was overrun");
        let events: Vec<_> = trace.iter().map(|(_, e)| e).collect();
        assert_eq!(
            events,
            vec![Event::Refused { count: 1 }],
            "a body was lost without a word"
        );
    }
    #[test]
    fn cancelled_requests_keep_bounded_receipts_until_the_boundary() {
        // Keep the owner live, proving explicit cancellation causes refusal.
        let mut world = crate::World::empty();
        let owner = world.load_scene(&crate::Scene::default()).unwrap();
        let mut scenes = world.scenes;
        let mut queue = SpawnQueue::default();
        for _ in 0..QUEUE_CAPACITY {
            queue
                .push_owned(Vec2::ZERO, crate::TEST_CHARACTER, Some(owner))
                .unwrap();
        }
        queue.cancel_scene(owner);
        assert_eq!(queue.len(), 0, "cancelled work reserves no body capacity");
        assert!(
            queue.push(Vec2::ZERO, crate::TEST_CHARACTER).is_none(),
            "outcome slots remain bounded until drain"
        );
        let mut enemies = Bodies::default();
        let mut trace = crate::Trace::default();
        drain(&mut queue, &mut enemies, &mut scenes, trace.sink(0));
        assert_eq!(queue.outcomes().len(), QUEUE_CAPACITY);
        assert!(
            queue
                .outcomes()
                .iter()
                .all(|result| result.entity.is_none())
        );
        assert!(queue.push(Vec2::ZERO, crate::TEST_CHARACTER).is_some());
    }

    /// Every placement door rejects an
    /// out-of-arena static body before identity or capability storage changes.
    #[test]
    fn invalid_static_grants_are_refused_without_mutation() {
        for (position, what) in [
            (Vec2::splat(crate::ARENA_HALF), Template::BLOCK),
            (Vec2::splat(f32::NAN), Template::BLOCK),
        ] {
            let mut world = crate::World::empty();
            let before = world.hash();
            assert!(world.bodies.spawn(position, what).is_none());
            assert_eq!(
                world.hash(),
                before,
                "storage itself must enforce admission"
            );
            assert!(world.place(position, what).is_none());
            assert_eq!(world.hash(), before);
            let scene = crate::Scene {
                name: "invalid".into(),
                bodies: vec![crate::Placed {
                    pos: position.into(),
                    what,
                }],
                ..crate::Scene::default()
            };
            assert!(world.load_scene(&scene).is_err());
            assert_eq!(world.hash(), before);
        }
    }
}
