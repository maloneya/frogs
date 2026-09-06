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
//! *ask* — a pass, the harness, a scenario, later a trigger — and exactly one
//! pass grants, first in the schedule and before anything reads a position.
//! What that buys, as facts rather than intentions:
//!
//! - the horde's length is **constant within a tick**, so a row index taken by
//!   one pass is still that body when the next pass runs;
//! - a request made *during* a tick becomes a body at the start of the next
//!   one, and a request made *between* ticks becomes a body during the next
//!   one — one deterministic latency, the same for every asker;
//! - the pending requests are simulation state, so they hash, they replay, and
//!   `state` reports them.
//!
//! ## The request is the abstraction, not the spawner
//!
//! Nothing here knows what asked. That is deliberate and it is the whole
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

use crate::hash::Fnv;
use crate::members::Members;
use crate::slots::EntityId;
use crate::trace::{Event, TraceSink};
use crate::{Enemies, MAX_ENEMIES};

/// How many requests may be pending at once.
///
/// Bounded so that a steady-state tick never touches the allocator, on the same
/// terms as the trace ring and the instance buffer. Generous next to anything
/// that can ask today, which is what makes hitting it a signal rather than a
/// routine event: a full queue means something is asking faster than the world
/// can absorb, and [`SpawnQueue::refused`] is what says so out loud.
pub(crate) const QUEUE_CAPACITY: usize = 256;
const _: () = assert!(QUEUE_CAPACITY > 0, "a zero-length queue would refuse everything, silently");

/// What to make, and what it should be able to do.
///
/// **Composed rather than configured.** A body is placed and then granted
/// behaviours; this is the written-down list of those grants, so "what kind of
/// enemy is this" stays a list of the things it does rather than a type. Adding
/// a behaviour adds a field here and a line in [`place`], and touches nothing
/// else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Template {
    /// Whether the body chases the player. See [`crate::pass::seek`].
    seeks: bool,
}

impl Template {
    /// A body and nothing else: it stands where it is put, and the solvers
    /// shove it around. What the horde has always been.
    pub const BODY: Self = Self { seeks: false };

    /// The same body, granted the chase behaviour.
    #[must_use]
    pub const fn seeking(self) -> Self {
        Self { seeks: true }
    }

    /// Whether this template chases. Read by [`place`], and by a scenario
    /// deciding what it just asked for.
    #[must_use]
    pub const fn seeks(self) -> bool {
        self.seeks
    }

    pub(crate) fn hash_into(self, h: &mut Fnv) {
        // Exhaustive, as every hash in this crate is: a behaviour added to the
        // template stops this compiling until it is fed in, and two pending
        // requests that differ only in what they will grant are different
        // simulation state.
        let Self { seeks } = self;
        h.usize(usize::from(seeks));
    }
}

/// One thing asked for, not yet made.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Request {
    /// Ground-plane position: `x` is world X and `y` is world **Z**.
    at: Vec2,
    what: Template,
}

/// What has been asked for and not yet granted.
///
/// FIFO, and drained in the order things were asked for. Not sorted, not
/// deduplicated, not batched by template: any of those would make the order in
/// which bodies take slots depend on something other than the order of the
/// requests, and slot order is what every id in a replay is keyed to.
pub(crate) struct SpawnQueue {
    pending: Vec<Request>,
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
        Self { pending: Vec::with_capacity(QUEUE_CAPACITY), refused: 0 }
    }
}

impl SpawnQueue {
    /// Asks for one thing. `false` when the queue is full, in which case the
    /// request is dropped and counted.
    ///
    /// Refusing rather than growing keeps the allocation promise, and refusing
    /// the *newest* rather than the oldest keeps a flood from starving requests
    /// that were already accepted.
    pub(crate) fn push(&mut self, at: Vec2, what: Template) -> bool {
        if self.pending.len() >= QUEUE_CAPACITY {
            self.refused += 1;
            return false;
        }
        self.pending.push(Request { at, what });
        true
    }

    /// How many requests are waiting. Reported in `state`, because "the spawn I
    /// asked for has not happened yet" and "the spawn I asked for was refused"
    /// look identical from the outside otherwise.
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }

    /// Forgets everything pending. For a wholesale reset of the horde, where a
    /// request decided before the reset would otherwise land a body a tick
    /// after it.
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.refused = 0;
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { pending, refused } = self;

        h.usize(pending.len());
        h.usize(*refused);
        for request in pending {
            h.f32(request.at.x);
            h.f32(request.at.y);
            request.what.hash_into(h);
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
/// Returns `None` when the horde is already at [`MAX_ENEMIES`] — a refusal
/// rather than a clamp, because the budget exists to stop the instance buffer
/// overrunning and an overrun is silent.
///
/// **Takes `&mut Enemies` where the pass contract asks for slices**, and that
/// is the one deliberate exception in the schedule. The pairing between
/// `slots`, `pos` and `prev_pos` — and the seeding of `prev_pos` that stops a
/// new body streaking across the arena for a frame — is an invariant the
/// storage owns. Handed three raw `Vec`s, this function would be reimplementing
/// that invariant at the one call site most likely to get it wrong.
pub(crate) fn place(
    enemies: &mut Enemies,
    seekers: &mut Members,
    at: Vec2,
    what: Template,
    trace: &mut TraceSink<'_>,
) -> Option<EntityId> {
    if enemies.len() >= MAX_ENEMIES {
        return None;
    }

    let id = enemies.spawn(at);

    // The grants. One line per behaviour, and the reason it is a line here
    // rather than a loop over some registry is the same reason `World::hash`
    // destructures: a behaviour added without a decision about whether a
    // template can grant it should be visible, not defaulted.
    if what.seeks {
        seekers.add(id);
    }

    // Per body rather than summarised. The trace's own rule reserves that for
    // things that happen rarely, which placement is *today*: the only askers
    // are setup and the harness, and both place a handful. The moment something
    // in the simulation can ask for a wave, this becomes a per-tick count — and
    // the golden traces will say so by churning.
    trace.emit(Event::Placed { id });

    Some(id)
}

/// Grants everything asked for since the last tick.
///
/// **First in the schedule**, so a body that comes into existence this tick
/// exists for the *whole* tick: it is remembered, it is walked past, it is
/// separated and it is contained, exactly like a body that was already there.
/// Draining anywhere later would create a body that exists for part of a tick,
/// which is a body whose first frame is a special case — and a special case in
/// a fixed-step loop is a bug with a schedule.
pub(crate) fn drain(
    queue: &mut SpawnQueue,
    enemies: &mut Enemies,
    seekers: &mut Members,
    mut trace: TraceSink<'_>,
) {
    // Requests refused at the door plus requests refused here are one number to
    // whoever asked: in both cases they asked and did not get it.
    let mut refused = core::mem::take(&mut queue.refused);

    // `drain` rather than iterate-then-clear: the queue must be empty
    // afterwards even if the horde is full, or a request refused this tick
    // would be retried on every tick forever.
    for request in queue.pending.drain(..) {
        if place(enemies, seekers, request.at, request.what, &mut trace).is_none() {
            refused += 1;
        }
    }

    if refused > 0 {
        trace.emit(Event::Refused { count: refused });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Enemies;

    /// **The refusal the instance budget exists for.** `MAX_ENEMIES` is
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
        let mut enemies = Enemies::default();
        enemies.respawn(MAX_ENEMIES);
        assert_eq!(enemies.len(), MAX_ENEMIES, "the horde did not actually fill");

        let mut seekers = Members::default();
        let mut queue = SpawnQueue::default();
        let mut trace = crate::Trace::default();

        assert!(queue.push(Vec2::ZERO, Template::BODY), "the queue refused before the horde could");
        drain(&mut queue, &mut enemies, &mut seekers, trace.sink(0));

        assert_eq!(enemies.len(), MAX_ENEMIES, "the budget was overrun");
        let events: Vec<_> = trace.iter().map(|(_, e)| e).collect();
        assert_eq!(events, vec![Event::Refused { count: 1 }], "a body was lost without a word");
    }
}
