//! Tick-stamped events: what the simulation did, as opposed to what it now
//! holds.
//!
//! `World::hash` and the `state` line are both *point samples* — they say where
//! everything is at the moment you looked. That is enough for movement and
//! useless for anything with a window. An attack that activates its hitbox one
//! tick early produces no crash, no compiler error, and a final state
//! indistinguishable from the correct one; the mistake exists only in the
//! interval between two samples, and a sample cannot see an interval.
//!
//! So this lands **before** the first hitbox rather than after, which is the
//! whole reason it is worth building while the only things to record are
//! contacts and wall hits.
//!
//! The shape deliberately mirrors `InstanceSink`, the other seam out of `sim`.
//! There, a pass can only `push` an `Instance` and cannot resize or reset the
//! buffer. Here, a pass gets a [`TraceSink`] that already knows the tick, so it
//! can only `emit` — **a pass physically cannot stamp an event with the wrong
//! tick**, which removes by construction the one error that would make every
//! timing assertion built on this worthless.

use core::fmt;

use crate::slots::EntityId;

/// How many events are kept.
///
/// A ring buffer rather than a growing log, because the game runs for hours and
/// the interesting window is always the last few seconds. At the current one-
/// event-per-pass-per-tick ceiling this is roughly a minute and a half of play.
const CAPACITY: usize = 16_384;
const _: () = assert!(CAPACITY > 0, "a zero-length trace silently records nothing");

/// Something worth knowing that happened during a tick.
///
/// **One event per pass per tick at most, summarising.** The temptation is to
/// emit per body — a `contact` for each of a thousand overlapping pairs — and
/// that floods the buffer so thoroughly that the rare events this exists for
/// scroll away before anyone reads them. Per-occurrence events belong to things
/// that occur rarely: a hitbox opening, a hit landing, hitstop starting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The horde was rebuilt. Not part of a tick — spawning happens outside the
    /// schedule, which is exactly why it is worth recording.
    Spawned {
        /// How many bodies the horde now holds.
        count: usize,
    },
    /// One body was deliberately placed. Distinct from `Spawned`, which is the
    /// whole horde being rebuilt: this is rare and individually interesting,
    /// so it names *which* body rather than how many.
    Placed {
        /// The body's new name.
        id: EntityId,
    },
    /// One body was removed, and its name retired.
    Removed {
        /// The name that just went stale. Anything still holding it will now
        /// resolve to nothing rather than to whichever body took its row.
        id: EntityId,
    },
    /// A swing began.
    ///
    /// The three attack events exist because the *interval* is the thing worth
    /// checking, and no point sample can see one. A hitbox that opens a tick
    /// early leaves final state indistinguishable from a correct swing; the
    /// only place the mistake exists is between two samples.
    Swung,
    /// The hitbox came into existence. Startup is over.
    HitboxOpened,
    /// The hitbox stopped existing. Anything arriving now is too late.
    HitboxClosed,
    /// A body was struck by the hitbox.
    ///
    /// Per body rather than summarised, which is the exception the trace's own
    /// rule allows for: a hit is rare, and *which* body was hit is the whole
    /// content of the event.
    Hit {
        /// The body the swing connected with.
        id: EntityId,
    },
    /// Overlapping pairs the solver pushed apart this tick.
    Contacts {
        /// Overlapping pairs resolved.
        count: usize,
    },
    /// Overlapping enemy pairs the crowd solver pushed apart this tick.
    ///
    /// Separate from `Contacts`, which is the player's own. They answer
    /// different questions — "is the character being jostled" and "is the crowd
    /// settling" — and the one number that used to be both could not tell a
    /// player wading into a pack from a pack shaking itself apart with nobody
    /// near it.
    Crowded {
        /// Enemy pairs resolved.
        count: usize,
    },
    /// Bodies the arena wall stopped this tick.
    Clamped {
        /// Bodies the wall stopped.
        count: usize,
    },
}

impl fmt::Display for Event {
    /// The golden-trace line format. Stable on purpose: a checked-in trace is a
    /// review artifact, and reformatting it would turn every future diff into
    /// noise that hides the one line that mattered.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawned { count } => write!(f, "spawned count={count}"),
            Self::Placed { id } => write!(f, "placed id={id}"),
            Self::Removed { id } => write!(f, "removed id={id}"),
            Self::Swung => write!(f, "swung"),
            Self::HitboxOpened => write!(f, "hitbox opened"),
            Self::HitboxClosed => write!(f, "hitbox closed"),
            Self::Hit { id } => write!(f, "hit id={id}"),
            Self::Contacts { count } => write!(f, "contacts count={count}"),
            Self::Crowded { count } => write!(f, "crowded count={count}"),
            Self::Clamped { count } => write!(f, "clamped count={count}"),
        }
    }
}

/// A ring buffer of tick-stamped events.
pub struct Trace {
    events: Vec<(u64, Event)>,
    /// Where the next event goes once `events` is full.
    next: usize,
    /// Events overwritten because the buffer wrapped.
    ///
    /// Reported rather than ignored, and the scenario runner refuses to compare
    /// a golden trace when it is nonzero. Silent truncation is the failure mode
    /// that would make a golden file quietly stop covering the beginning of the
    /// run it claims to describe.
    dropped: usize,
}

impl Default for Trace {
    fn default() -> Self {
        // Allocated once, up front, for the same reason the instance buffer is:
        // a steady-state tick must not touch the allocator.
        Self { events: Vec::with_capacity(CAPACITY), next: 0, dropped: 0 }
    }
}

impl Trace {
    /// Hands out the only thing a pass may write through.
    pub(crate) fn sink(&mut self, tick: u64) -> TraceSink<'_> {
        TraceSink { trace: self, tick }
    }

    /// Throws away everything recorded so far.
    ///
    /// For the seam between *setting a world up* and *running it*. A scenario's
    /// golden trace should describe the run; without this it also describes
    /// `World::default()` building a horde the scenario immediately replaces,
    /// which couples every golden file in the repository to `DEFAULT_ENEMIES`.
    /// An unrelated edit would then break all of them at once, and a gate that
    /// cries wolf is a gate somebody switches off.
    pub fn clear(&mut self) {
        self.events.clear();
        self.next = 0;
        self.dropped = 0;
    }

    /// Events in the order they happened, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = (u64, Event)> + '_ {
        let (old, new) = self.events.split_at(self.next.min(self.events.len()));
        new.iter().chain(old).copied()
    }

    /// Everything from `tick` onward. What the harness's `trace since` serves.
    pub fn since(&self, tick: u64) -> impl Iterator<Item = (u64, Event)> + '_ {
        self.iter().filter(move |(t, _)| *t >= tick)
    }

    /// How many events fell off the back. Nonzero means the buffer wrapped and
    /// the beginning of the run is gone.
    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Renders the whole trace one event per line, `<tick> <event>`.
    ///
    /// This is the golden-file format, so it lives here beside `Display` rather
    /// than in the scenario runner — a second place that knew how to format an
    /// event would be a second thing to keep in step.
    #[must_use]
    pub fn render(&self) -> String {
        use fmt::Write as _;
        let mut out = String::new();
        for (tick, event) in self.iter() {
            let _ = writeln!(out, "{tick} {event}");
        }
        out
    }

    fn push(&mut self, tick: u64, event: Event) {
        if self.events.len() < CAPACITY {
            self.events.push((tick, event));
            // `next` only means anything once the buffer is full; until then the
            // events are already in order and `split_at` below handles it.
            self.next = 0;
        } else {
            self.events[self.next] = (tick, event);
            self.next = (self.next + 1) % CAPACITY;
            self.dropped += 1;
        }
    }
}

/// The write end of the trace, already bound to a tick.
///
/// A pass declares one of these in its signature the way it declares any other
/// slice it touches, and gets no other access to the trace: it cannot read the
/// history, cannot reset it, and cannot stamp an event with a tick other than
/// the one being run.
pub(crate) struct TraceSink<'a> {
    trace: &'a mut Trace,
    tick: u64,
}

impl TraceSink<'_> {
    /// Records an event at the current tick.
    pub(crate) fn emit(&mut self, event: Event) {
        self.trace.push(self.tick, event);
    }

    /// Re-borrows, so one tick's sink can be handed to several passes in turn.
    pub(crate) fn reborrow(&mut self) -> TraceSink<'_> {
        TraceSink { trace: self.trace, tick: self.tick }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_come_back_in_the_order_they_happened() {
        let mut trace = Trace::default();
        for tick in 0..5 {
            trace.sink(tick).emit(Event::Contacts { count: tick as usize });
        }

        let got: Vec<_> = trace.iter().collect();
        assert_eq!(got.len(), 5);
        for (i, (tick, event)) in got.iter().enumerate() {
            assert_eq!(*tick, i as u64);
            assert_eq!(*event, Event::Contacts { count: i });
        }
    }

    #[test]
    fn since_starts_where_it_is_asked_to() {
        let mut trace = Trace::default();
        for tick in 0..10 {
            trace.sink(tick).emit(Event::Clamped { count: 1 });
        }

        let got: Vec<_> = trace.since(7).collect();
        assert_eq!(got.len(), 3, "since(7) should hand back ticks 7, 8 and 9");
        assert_eq!(got[0].0, 7);
    }

    /// Wrapping has to stay ordered *and* has to be visible. An overwritten
    /// event that nobody is told about is how a golden trace quietly stops
    /// describing the run it is named after.
    #[test]
    fn wrapping_keeps_order_and_is_reported() {
        let mut trace = Trace::default();
        let overflow = 10;

        for tick in 0..(CAPACITY + overflow) as u64 {
            trace.sink(tick).emit(Event::Contacts { count: 0 });
        }

        assert_eq!(trace.dropped(), overflow, "the buffer wrapped without saying so");

        let got: Vec<_> = trace.iter().collect();
        assert_eq!(got.len(), CAPACITY);
        assert_eq!(got[0].0, overflow as u64, "the oldest surviving event is not the oldest kept");
        assert_eq!(got[CAPACITY - 1].0, (CAPACITY + overflow - 1) as u64);

        let ticks: Vec<u64> = got.iter().map(|(t, _)| *t).collect();
        assert!(ticks.windows(2).all(|w| w[0] < w[1]), "wrapping scrambled the order");
    }

    /// The golden-file format. If this test is updated, every checked-in trace
    /// has to be re-blessed, which is the cost of changing it and the reason
    /// not to do so lightly.
    #[test]
    fn the_rendered_format_is_tick_then_event() {
        let mut trace = Trace::default();
        trace.sink(0).emit(Event::Spawned { count: 64 });
        trace.sink(3).emit(Event::Contacts { count: 2 });
        trace.sink(3).emit(Event::Clamped { count: 1 });

        assert_eq!(trace.render(), "0 spawned count=64\n3 contacts count=2\n3 clamped count=1\n");
    }
}
