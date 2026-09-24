//! Bringing new things into the world, at exactly one point in the tick.
//!
//! ## Why a queue rather than a function anything may call
//!
//! Spawning is *structural*: it pushes rows onto the horde's arrays, and a
//! despawn swap-removes them. Every other pass holds `&mut [Vec2]` into those
//! arrays, and [`crate::pass::seek`] resolves an id to a row and then indexes
//! with it. So a body appearing halfway through a tick does not merely
//! reallocate a slice — it moves rows out from under an index that has already
//! been taken. There is no ordering of the schedule that makes "any pass may
//! spawn" safe.
//!
//! A queue removes the problem rather than defending against it. Anything may
//! *ask* — a source, the harness, a scenario — and exactly one pass grants,
//! before anything else holds a row.
//! What that buys, as facts rather than intentions:
//!
//! - the horde's length is constant until the final defeat boundary, so a row
//!   index taken by one ordinary pass is still that body when the next runs;
//! - a request made *during* a tick becomes a body at the start of the next
//!   one, and a request made *between* ticks becomes a body during the next
//!   one — one deterministic latency, the same for every asker;
//! - the pending requests are simulation state, so they hash, they replay, and
//!   `state` reports them.
//!
//! ## The request is the abstraction, not the spawner
//!
//! Requests carry optional producer identity for outcome attribution, but no
//! producer behaviour. That separation is the whole
//! extension point: a "spawner" is not a kind of entity that owns this
//! machinery, it is *anything that pushes a request*. A wave director, a body
//! that splits when killed, and a debug key are the same thing to this module.
//! Whether a trigger has a position, whether it can itself be destroyed, and
//! how it decides — none of it reaches in here.
//!
//! ## What a [`Template`] is
//!
//! What to make, as data. Today the only thing this crate can make is a body,
//! and what varies between two kinds of enemy is the list of behaviours it is
//! granted — so that is what a template holds. When a second *kind* of thing
//! exists (a projectile, a pickup), this gains a discriminant and the queue and
//! the drain below do not change shape.
//!
//! Data rather than a closure, for three reasons that are all load-bearing
//! here: `World::hash` destructures exhaustively and a `Box<dyn Fn>` cannot be
//! fed to it, a scenario deserialises its spawns from RON, and a replay has to
//! reproduce the decision exactly.

use glam::Vec2;
use serde::Deserialize;

use crate::hash::Fnv;
use crate::members::Members;
use crate::scene::Scenes;
use crate::slots::EntityId;
use crate::trace::{Event, TraceSink};
use crate::{Bodies, SceneId};
#[cfg(test)]
use crate::MAX_BODIES;

/// How many requests may be pending at once.
///
/// Bounded so that a steady-state tick never touches the allocator, on the same
/// terms as the trace ring and the instance buffer. Generous next to anything
/// that can ask today, which is what makes hitting it a signal rather than a
/// routine event: a full queue means something is asking faster than the world
/// can absorb, and [`SpawnQueue::refused`] is what says so out loud.
/// Cancelled scene requests retain their slots until the next boundary, so
/// cancellation cannot create an unbounded number of outstanding outcomes.
pub(crate) const QUEUE_CAPACITY: usize = 256;
const _: () = assert!(QUEUE_CAPACITY > 0, "a zero-length queue would refuse everything, silently");

/// What to make, and what it should be able to do.
///
/// **Composed rather than configured.** A body is placed and then granted
/// behaviours; this is the written-down list of those grants, so "what kind of
/// enemy is this" stays a list of the things it does rather than a type. Adding
/// a behaviour adds a field here and a line in [`place`], and touches nothing
/// else.
///
/// **This is also the written form.** A scenario names a template in RON as
/// `(seeks: true)`, straight into this type, so a behaviour added here is
/// describable from outside on the same edit. The alternative — a mirror struct
/// in whichever crate reads the file — costs an edit per behaviour per copy and
/// fails in the direction nothing reports: a forgotten copy leaves the new
/// behaviour unreachable rather than breaking a build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Template {
    /// Whether the body chases the player. See [`crate::pass::seek`].
    seeks: bool,
    /// Immovable and immune to damage; uses the shared body footprint.
    fixed: bool,
    /// Grants a one-shot Ready to Activated interaction.
    interactable: bool,
}

impl Template {
    /// A body and nothing else: it stands where it is put, and the solvers
    /// shove it around. What the horde has always been.
    pub const BODY: Self = Self { seeks: false, fixed: false, interactable: false };

    /// The same movable body, granted the chase behaviour.
    /// Panics for static props, which cannot acquire powered movement.
    #[must_use]
    pub const fn seeking(self) -> Self {
        assert!(!self.fixed, "a static body cannot chase");
        Self { seeks: true, ..self }
    }

    /// Whether this template chases. Read by [`place`], and by a scenario
    /// deciding what it just asked for.
    #[must_use]
    pub const fn seeks(self) -> bool {
        self.seeks
    }

    /// A static prop, with no chase or damage capability.
    pub const BLOCK: Self = Self { seeks: false, fixed: true, interactable: false };

    /// Grants interaction independently of physical movement or health.
    #[must_use]
    pub const fn interactive(self) -> Self {
        Self { interactable: true, ..self }
    }

    /// Whether the body supports interaction.
    #[must_use]
    pub const fn interactable(self) -> bool {
        self.interactable
    }

    /// Whether the body is immovable.
    #[must_use]
    pub const fn fixed(self) -> bool {
        self.fixed
    }

    /// Refuses contradictory grants and static placements outside the arena.
    /// Shared by scene admission and the only spawn door.
    pub(crate) fn valid_at(self, at: Vec2) -> bool {
        at.is_finite()
            && (!self.fixed
                || (!self.seeks
                    && at.abs().max_element() <= crate::ARENA_HALF - crate::ENEMY_RADIUS))
    }

    pub(crate) fn hash_into(self, h: &mut Fnv) {
        // Exhaustive, as every hash in this crate is: a behaviour added to the
        // template stops this compiling until it is fed in, and two pending
        // requests that differ only in what they will grant are different
        // simulation state.
        let Self { seeks, fixed, interactable } = self;
        h.usize(usize::from(interactable));
        h.usize(usize::from(fixed));
        h.usize(usize::from(seeks));
    }
}

/// Stable identity for an accepted queued spawn, scoped to this world lifetime.
/// Scene eviction never reuses tickets; restarting creates a fresh lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnTicket(u64);

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
        Self { pending: Vec::with_capacity(QUEUE_CAPACITY), next_ticket: 0,
            outcomes: Vec::with_capacity(QUEUE_CAPACITY), refused: 0 }
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

    pub(crate) fn push_owned(&mut self, at: Vec2, what: Template, owner: Option<SceneId>) -> Option<SpawnTicket> {
        self.push_from(at, what, owner, None)
    }

    pub(crate) fn push_from(&mut self, at: Vec2, what: Template, owner: Option<SceneId>, source: Option<crate::SourceId>) -> Option<SpawnTicket> {
        if self.pending.len() >= QUEUE_CAPACITY {
            self.refused += 1;
            return None;
        }
        let ticket = SpawnTicket(self.next_ticket);
        self.next_ticket = self.next_ticket.checked_add(1).expect("spawn ticket space exhausted");
        self.pending.push(Request { ticket, cancelled: false, at, what, owner, source });
        Some(ticket)
    }

    /// How many requests are waiting. Reported in `state`, because "the spawn I
    /// asked for has not happened yet" and "the spawn I asked for was refused"
    /// look identical from the outside otherwise.
    pub(crate) fn len(&self) -> usize {
        self.pending.iter().filter(|request| !request.cancelled).count()
    }

    pub(crate) fn outcomes(&self) -> &[SpawnOutcome] { &self.outcomes }

    pub(crate) fn cancel_scene(&mut self, owner: SceneId) {
        // Keep the bounded request slot until the boundary so every accepted
        // ticket receives a result, even after scene eviction.
        for request in &mut self.pending {
            if request.owner == Some(owner) { request.cancelled = true; }
        }
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { pending, next_ticket, outcomes, refused } = self;
        h.u64(*next_ticket);
        h.usize(outcomes.len());
        for outcome in outcomes {
            let SpawnOutcome { ticket, entity, owner, source } = outcome;
            h.u64(ticket.0);
            h.usize(usize::from(entity.is_some()));
            if let Some(id) = entity { id.hash_into(h); }
            h.usize(usize::from(owner.is_some()));
            if let Some(id) = owner { id.hash_into(h); }
            h.usize(usize::from(source.is_some()));
            if let Some(id) = source { id.hash_into(h); }
        }

        h.usize(pending.len());
        h.usize(*refused);
        for request in pending {
            let Request { ticket, cancelled, at, what, owner, source } = request;
            h.u64(ticket.0);
            h.usize(usize::from(*cancelled));
            h.f32(at.x);
            h.f32(at.y);
            what.hash_into(h);
            h.usize(usize::from(source.is_some()));
            if let Some(source) = source { source.hash_into(h); }
            h.usize(usize::from(owner.is_some()));
            if let Some(owner) = owner {
                owner.hash_into(h);
            }
        }
    }
}

/// Puts one body in the world and grants it what its template says.
///
/// **The single door.** Immediate placement (scenario setup, the harness) and
/// queued placement both come through here, so "what a template means" is one
/// function rather than a rule two call sites are trusted to apply the same
/// way.
///
/// Returns `None` when the horde is already at [`crate::MAX_BODIES`] — a refusal
/// rather than a clamp, because the budget exists to stop the instance buffer
/// overrunning and an overrun is silent.
///
/// **Takes `&mut Bodies` where the pass contract asks for slices**, and that
/// is the one deliberate exception in the schedule. The pairing between
/// `slots`, `pos` and `prev_pos` — and the seeding of `prev_pos` that stops a
/// new body streaking across the arena for a frame — is an invariant the
/// storage owns. Handed three raw `Vec`s, this function would be reimplementing
/// that invariant at the one call site most likely to get it wrong.
pub(crate) fn place(
    enemies: &mut Bodies,
    seekers: &mut Members,
    at: Vec2,
    what: Template,
    trace: &mut TraceSink<'_>,
) -> Option<EntityId> {
    let id = enemies.spawn(at, what)?;

    // The grants. One line per behaviour, and the reason it is a line here
    // rather than a loop over some registry is the same reason `World::hash`
    // destructures: a behaviour added without a decision about whether a
    // template can grant it should be visible, not defaulted.
    if what.seeks {
        seekers.add(id);
    }

    // Diagnostics describe placement; authoritative outcomes bind request identities.
    trace.emit(Event::Placed { id });

    Some(id)
}

/// Grants everything asked for since the last tick.
///
/// **Before anything holds a row**, so a body that comes into existence this tick
/// exists for the *whole* tick: it is remembered, it is walked past, it is
/// separated and it is contained, exactly like a body that was already there.
/// Draining anywhere later would create a body that exists for part of a tick,
/// which is a body whose first frame is a special case — and a special case in
/// a fixed-step loop is a bug with a schedule.
pub(crate) fn drain(
    queue: &mut SpawnQueue,
    enemies: &mut Bodies,
    seekers: &mut Members,
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
            place(enemies, seekers, request.at, request.what, &mut trace)
        };
        if let Some(id) = entity {
            enemies.health.set_origin(id, request.source);
            if let Some(owner) = request.owner { scenes.record_body(owner, id); }
        } else {
            refused += 1;
        }
        queue.outcomes.push(SpawnOutcome {
            ticket: request.ticket, entity, owner: request.owner, source: request.source,
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

    /// **The refusal the instance budget exists for.** `MAX_BODIES` is
    /// whatever the ground and the player leave of the instance buffer, and an
    /// overrun is silent — the upload truncates and bodies simply stop being
    /// drawn. So a request that arrives at a full horde has to be refused
    /// loudly, and the drain is the door that has to do it: nothing else sees
    /// the request at all.
    ///
    /// Calls `drain` directly rather than stepping a world, and that is
    /// deliberate. Separation is brute force and quadratic, so one tick of a
    /// full horde is seconds; this test is about a length check, and paying for
    /// the solver to prove it would be a test nobody runs.
    #[test]
    fn a_full_horde_refuses_what_it_cannot_hold() {
        let mut enemies = Bodies::default();
        for _ in 0..MAX_BODIES {
            enemies.spawn(Vec2::ZERO, Template::BODY).unwrap();
        }
        assert_eq!(enemies.len(), MAX_BODIES + 1, "the horde did not actually fill");

        let mut seekers = Members::default();
        let mut queue = SpawnQueue::default();
        let mut trace = crate::Trace::default();

        assert!(queue.push(Vec2::ZERO, Template::BODY).is_some(), "the queue refused before the horde could");
        drain(&mut queue, &mut enemies, &mut seekers, &mut Scenes::default(), trace.sink(0));

        assert_eq!(enemies.len(), MAX_BODIES + 1, "the budget was overrun");
        let events: Vec<_> = trace.iter().map(|(_, e)| e).collect();
        assert_eq!(events, vec![Event::Refused { count: 1 }], "a body was lost without a word");
    }
    #[test]
    fn cancelled_requests_keep_bounded_receipts_until_the_boundary() {
        // Keep the owner live, proving explicit cancellation causes refusal.
        let mut world = crate::World::empty();
        let owner = world.load_scene(&crate::Scene::default()).unwrap();
        let mut scenes = world.scenes;
        let mut queue = SpawnQueue::default();
        for _ in 0..QUEUE_CAPACITY {
            queue.push_owned(Vec2::ZERO, Template::BODY, Some(owner)).unwrap();
        }
        queue.cancel_scene(owner);
        assert_eq!(queue.len(), 0, "cancelled work reserves no body capacity");
        assert!(queue.push(Vec2::ZERO, Template::BODY).is_none(), "outcome slots remain bounded until drain");
        let mut enemies = Bodies::default();
        let mut seekers = Members::default();
        let mut trace = crate::Trace::default();
        drain(&mut queue, &mut enemies, &mut seekers, &mut scenes, trace.sink(0));
        assert_eq!(queue.outcomes().len(), QUEUE_CAPACITY);
        assert!(queue.outcomes().iter().all(|result| result.entity.is_none()));
        assert!(queue.push(Vec2::ZERO, Template::BODY).is_some());
    }

    /// Every placement door rejects immovable/chasing contradictions and an
    /// out-of-arena static body before identity or capability storage changes.
    #[test]
    fn invalid_static_grants_are_refused_without_mutation() {
        let invalid = Template { seeks: true, ..Template::BLOCK };
        for (position, what) in [
            (Vec2::Y, invalid),
            (Vec2::splat(crate::ARENA_HALF), Template::BLOCK),
            (Vec2::splat(f32::NAN), Template::BLOCK),
        ] {
            let mut world = crate::World::empty();
            let before = world.hash();
            assert!(world.bodies.spawn(position, what).is_none());
            assert_eq!(world.hash(), before, "storage itself must enforce admission");
            assert!(world.place(position, what).is_none());
            assert_eq!(world.hash(), before);
            let scene = crate::Scene { name: "invalid".into(), bodies: vec![crate::Placed {
                pos: position.into(), what,
            }], ..crate::Scene::default() };
            assert!(world.load_scene(&scene).is_err());
            assert_eq!(world.hash(), before);
        }
    }

}
