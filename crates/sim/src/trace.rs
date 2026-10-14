//! Tick-stamped diagnostic events. [`TraceSink`] supplies the tick and permits
//! emission without exposing the trace buffer to passes.

use core::fmt;

use crate::pass::source::SourceId;
use crate::slots::EntityId;

/// How many events are kept.
///
/// A ring buffer rather than a growing log, because the game runs for hours and
/// the interesting window is recent history. The default keeps four times the
/// old 16K history; capture correctness never depends on this retention size.
const CAPACITY: usize = 65_536;
/// Maximum events per history or assertion capture. Shared by every stream so
/// callers cannot request unbounded allocations; zero remains valid.
pub const MAX_TRACE_EVENTS: usize = 262_144;
const _: () = assert!(CAPACITY > 0, "a zero-length trace silently records nothing");

/// Diagnostic transitions. Aggregate repetitive contacts; retain identities for hits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Accepted type-wide edit before the named tick.
    TuningChanged {
        /// Attribute edited.
        field: crate::TuningField,
        /// Accepted value.
        value: f32,
    },
    /// An explicit action applied damage to a live target.
    Damaged {
        /// Actor responsible for the effect.
        actor: EntityId,
        /// Identity receiving damage.
        target: EntityId,
        /// Health after this effect.
        remaining: u8,
    },
    /// A nearby object changed from Ready to Activated.
    Activated {
        /// The single object activated by this press.
        id: EntityId,
    },
    /// An entire scene became ready between ticks.
    SceneLoaded {
        /// Runtime instance, distinct from the authored name.
        id: crate::SceneId,
    },
    /// An instance and all of its remaining work were removed.
    SceneEvicted {
        /// Retired runtime instance.
        id: crate::SceneId,
    },
    /// Recovery tuning changed before this tick. In-flight swings keep theirs.
    AttackRecoveryChanged {
        /// Validated duration used by subsequent swings.
        recovery: crate::RecoveryTicks,
    },
    /// Authored attack content changed between ticks.
    AttackProfileChanged {
        /// Profile used by subsequent swings.
        profile: crate::AttackProfile,
    },
    /// A physical impulse was applied; source is absent for external commands.
    Impulsed {
        /// Recipient's stable identity.
        id: EntityId,
        /// Entity that caused this impulse, if any.
        source: Option<EntityId>,
        /// Momentum added in world X/Z.
        impulse: crate::Impulse,
    },
    /// A joint's lock failed under load; the body keeps its joint.
    LockBroken {
        /// The jointed body's stable identity.
        id: EntityId,
    },
    /// A toppling pole reached the ground and lies flat.
    Toppled {
        /// The jointed body's stable identity.
        id: EntityId,
    },
    /// Contact responses that exchanged momentum this tick, aggregated.
    Momentum {
        /// Number of closing pairs resolved.
        pairs: usize,
    },
    /// Bodies whose carried velocity reached rest this tick.
    Rested {
        /// Number of bodies that stopped.
        count: usize,
    },
    /// One body was deliberately placed.
    Placed {
        /// The body's new name.
        id: EntityId,
    },
    /// A source was added, or removed.
    ///
    /// Traced because both happen *outside* a tick, which is the hardest kind
    /// of state change to account for when reading a trace afterwards — and
    /// because a source is the explanation for every body it goes on to make.
    /// A trace that shows bodies appearing with no record of what was asked to
    /// make them is a trace that answers the wrong question.
    SourceAdded {
        /// The source's name, for the life of the world.
        id: SourceId,
    },
    /// A source was removed. Nothing it would have made will be made.
    SourceRemoved {
        /// The name that just stopped resolving. Ids are never recycled, so it
        /// resolves to nothing rather than to a source added later.
        id: SourceId,
    },
    /// A live source's enablement changed; repeated settings emit nothing.
    SourceEnablementChanged {
        /// Stable source identity.
        id: SourceId,
        /// Whether its cadence may now advance.
        enabled: bool,
    },
    /// A source asked for a body.
    ///
    /// **The causal half of a spawn**, and the reason it is separate from
    /// `Placed`: that one says a body appeared, this one says who asked. With
    /// two sources running, nothing else in the trace can tell their output
    /// apart.
    Fired {
        /// Which source asked.
        source: SourceId,
    },
    /// Number of spawn requests refused this tick.
    Refused {
        /// Requests dropped: the queue was full, or the horde was already at
        /// the instance budget.
        count: usize,
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
    /// A held beam began.
    ChannelStarted,
    /// Release or selection change ended a beam.
    ChannelStopped,
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
        /// Hits left after the connection; zero is removed later in this tick.
        remaining: u8,
    },
    /// Player pairs overlapping more than a millimetre when this tick's
    /// contacts were first collided.
    Contacts {
        /// Overlapping pairs.
        count: usize,
    },
    /// Non-player pairs overlapping more than a millimetre when this tick's
    /// contacts were first collided.
    ///
    /// Separate from `Contacts`, which is the player's own. They answer
    /// different questions — "is the character being jostled" and "is the crowd
    /// settling" — and one number covering both could not tell a player wading
    /// into a pack from a pack shaking itself apart with nobody near it.
    Crowded {
        /// Overlapping pairs.
        count: usize,
    },
    /// Movable bodies and hinges that exchanged momentum with a wall this tick.
    WallContacts {
        /// Bodies pressed against a wall.
        count: usize,
    },
    /// Bodies the arena wall stopped this tick.
    Clamped {
        /// Bodies the wall stopped.
        count: usize,
    },
}

/// Event categories for diagnostic selection; payloads remain in [`Event`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum EventKind {
    /// Explicit action damage.
    Damaged,
    /// A runtime tuning edit.
    TuningChanged,
    /// The `Activated` transition.
    Activated,
    /// The `SceneLoaded` transition.
    SceneLoaded,
    /// The `SceneEvicted` transition.
    SceneEvicted,
    /// The `AttackRecoveryChanged` transition.
    AttackRecoveryChanged,
    /// The `AttackProfileChanged` transition.
    AttackProfileChanged,
    /// The `Impulsed` transition.
    Impulsed,
    /// The `LockBroken` transition.
    LockBroken,
    /// The `Toppled` transition.
    Toppled,
    /// The `Momentum` transition.
    Momentum,
    /// The `Rested` transition.
    Rested,
    /// The `Placed` transition.
    Placed,
    /// The `SourceAdded` transition.
    SourceAdded,
    /// The `SourceRemoved` transition.
    SourceRemoved,
    /// The `SourceEnablementChanged` transition.
    SourceEnablementChanged,
    /// The `Fired` transition.
    Fired,
    /// The `Refused` transition.
    Refused,
    /// The `Removed` transition.
    Removed,
    /// The `Swung` transition.
    Swung,
    /// The `ChannelStarted` transition.
    ChannelStarted,
    /// The `ChannelStopped` transition.
    ChannelStopped,
    /// The `HitboxOpened` transition.
    HitboxOpened,
    /// The `HitboxClosed` transition.
    HitboxClosed,
    /// The `Hit` transition.
    Hit,
    /// The `Contacts` transition.
    Contacts,
    /// The `Crowded` transition.
    Crowded,
    /// The `WallContacts` transition.
    WallContacts,
    /// The `Clamped` transition.
    Clamped,
}

impl Event {
    /// Category without discarding the original event payload.
    #[must_use]
    pub fn kind(self) -> EventKind {
        match self {
            Self::Damaged { .. } => EventKind::Damaged,
            Self::TuningChanged { .. } => EventKind::TuningChanged,
            Self::Activated { .. } => EventKind::Activated,
            Self::SceneLoaded { .. } => EventKind::SceneLoaded,
            Self::SceneEvicted { .. } => EventKind::SceneEvicted,
            Self::AttackRecoveryChanged { .. } => EventKind::AttackRecoveryChanged,
            Self::AttackProfileChanged { .. } => EventKind::AttackProfileChanged,
            Self::Impulsed { .. } => EventKind::Impulsed,
            Self::LockBroken { .. } => EventKind::LockBroken,
            Self::Toppled { .. } => EventKind::Toppled,
            Self::Momentum { .. } => EventKind::Momentum,
            Self::Rested { .. } => EventKind::Rested,
            Self::Placed { .. } => EventKind::Placed,
            Self::SourceAdded { .. } => EventKind::SourceAdded,
            Self::SourceRemoved { .. } => EventKind::SourceRemoved,
            Self::SourceEnablementChanged { .. } => EventKind::SourceEnablementChanged,
            Self::Fired { .. } => EventKind::Fired,
            Self::Refused { .. } => EventKind::Refused,
            Self::Removed { .. } => EventKind::Removed,
            Self::Swung => EventKind::Swung,
            Self::ChannelStarted => EventKind::ChannelStarted,
            Self::ChannelStopped => EventKind::ChannelStopped,
            Self::HitboxOpened => EventKind::HitboxOpened,
            Self::HitboxClosed => EventKind::HitboxClosed,
            Self::Hit { .. } => EventKind::Hit,
            Self::Contacts { .. } => EventKind::Contacts,
            Self::Crowded { .. } => EventKind::Crowded,
            Self::WallContacts { .. } => EventKind::WallContacts,
            Self::Clamped { .. } => EventKind::Clamped,
        }
    }
}

impl fmt::Display for Event {
    /// The golden-trace line format. Stable on purpose: a checked-in trace is a
    /// review artifact, and reformatting it would turn every future diff into
    /// noise that hides the one line that mattered.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Damaged { actor, target, remaining } => write!(f, "damaged actor={actor} target={target} remaining={remaining}"),
            Self::TuningChanged { field, value } => write!(f, "tuning {}={value}", field.attribute().key),
            Self::Activated { id } => write!(f, "activated id={id}"),
            Self::SceneLoaded { id } => write!(f, "scene loaded id={id}"),
            Self::SceneEvicted { id } => write!(f, "scene evicted id={id}"),
            Self::AttackRecoveryChanged { recovery } => {
                write!(f, "attack recovery ticks={}", recovery.get())
            }
            Self::AttackProfileChanged { profile } => {
                write!(f, "attack profile={}", profile.label())
            }
            Self::Impulsed { id, source, impulse } => {
                write!(f, "impulse id={id} value={impulse} source=")?;
                match source {
                    Some(id) => write!(f, "{id}"),
                    None => write!(f, "external"),
                }
            }
            Self::LockBroken { id } => write!(f, "lock broken id={id}"),
            Self::Toppled { id } => write!(f, "toppled id={id}"),
            Self::Momentum { pairs } => write!(f, "momentum pairs={pairs}"),
            Self::Rested { count } => write!(f, "rested count={count}"),
            Self::Placed { id } => write!(f, "placed id={id}"),
            Self::SourceAdded { id } => write!(f, "source added id={id}"),
            Self::SourceRemoved { id } => write!(f, "source removed id={id}"),
            Self::SourceEnablementChanged { id, enabled } => {
                write!(f, "source enabled id={id} value={enabled}")
            }
            Self::Fired { source } => write!(f, "fired source={source}"),
            Self::Refused { count } => write!(f, "refused count={count}"),
            Self::Removed { id } => write!(f, "removed id={id}"),
            Self::ChannelStarted => write!(f, "channel started"),
            Self::ChannelStopped => write!(f, "channel stopped"),
            Self::Swung => write!(f, "swung"),
            Self::HitboxOpened => write!(f, "hitbox opened"),
            Self::HitboxClosed => write!(f, "hitbox closed"),
            Self::Hit { id, remaining } => write!(f, "hit id={id} remaining={remaining}"),
            Self::Contacts { count } => write!(f, "contacts count={count}"),
            Self::Crowded { count } => write!(f, "crowded count={count}"),
            Self::WallContacts { count } => write!(f, "wall contacts count={count}"),
            Self::Clamped { count } => write!(f, "clamped count={count}"),
        }
    }
}

/// A ring buffer of tick-stamped events.
pub struct Trace<E = Event> {
    events: Vec<(u64, E)>,
    capacity: usize,
    capture: Option<Capture<E>>,
    /// Where the next event goes once `events` is full.
    next: usize,
    /// Events overwritten because the buffer wrapped.
    ///
    /// Reports lost live debugging history. Assertion capture has a separate
    /// fail-closed bound and never reads this ring.
    dropped: usize,
}

impl<E> Default for Trace<E> {
    fn default() -> Self {
        // Allocated once, up front, for the same reason the instance buffer is:
        // a steady-state tick must not touch the allocator.
        Self { events: Vec::with_capacity(CAPACITY), capacity: CAPACITY, capture: None, next: 0, dropped: 0 }
    }
}

struct Capture<E> {
    events: Vec<(u64, E)>,
    limit: usize,
    overflowed: bool,
    accepts: Box<dyn Fn(E) -> bool + Send + Sync>,
}

impl<E: Copy + fmt::Display> Trace<E> {
    /// Change recent-history retention. Zero disables history. Does not clear
    /// an independent observation capture or affect simulation state. Limits
    /// above [`MAX_TRACE_EVENTS`] are refused without mutation.
    pub fn set_retention(&mut self, capacity: usize) -> Result<(), &'static str> {
        Self::validate_limit(capacity)?;
        self.capacity = capacity;
        self.events = Vec::with_capacity(capacity);
        self.next = 0;
        self.dropped = 0;
        Ok(())
    }

    /// Reject unsupported allocations before changing observation state.
    pub fn validate_limit(limit: usize) -> Result<(), &'static str> {
        if limit > MAX_TRACE_EVENTS { Err("trace event capacity exceeds MAX_TRACE_EVENTS") } else { Ok(()) }
    }

    /// Start a bounded, filtered observation capture independent of the live
    /// ring. Exhaustion is explicit and never overwrites required evidence.
    /// Limits above [`MAX_TRACE_EVENTS`] preserve any current capture.
    pub fn capture(&mut self, limit: usize, accepts: impl Fn(E) -> bool + Send + Sync + 'static) -> Result<(), &'static str> {
        Self::validate_limit(limit)?;
        self.capture = Some(Capture {
            events: Vec::with_capacity(limit), limit, overflowed: false,
            accepts: Box::new(accepts),
        });
        Ok(())
    }

    /// Stop independent observation collection and release its storage.
    pub fn stop_capture(&mut self) { self.capture = None; }

    /// Complete captured evidence, or `None` if not enabled or exhausted.
    pub fn captured(&self) -> Option<&[(u64, E)]> {
        self.capture.as_ref().filter(|capture| !capture.overflowed)
            .map(|capture| capture.events.as_slice())
    }

    /// Hands out the only thing a pass may write through.
    pub fn sink(&mut self, tick: u64) -> TraceSink<'_, E> {
        TraceSink { trace: self, tick }
    }

    /// Throws away everything recorded so far.
    ///
    /// For the seam between *setting a world up* and *running it*. A scenario's
    /// golden trace should describe the run; without this it also describes
    /// authored setup placements. Clearing at this boundary keeps the trace
    /// focused on transitions during the scenario's scheduled ticks.
    pub fn clear(&mut self) {
        self.events.clear();
        self.next = 0;
        self.dropped = 0;
    }

    /// How many events are held. O(1) — `iter().count()` walks the whole ring.
    ///
    /// Crate-visible: `World::report` is the only caller, and a `pub` `len`
    /// obliges an `is_empty` nobody would call.
    pub(crate) fn len(&self) -> usize {
        self.events.len()
    }

    /// Events in the order they happened, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = (u64, E)> + '_ {
        let (old, new) = self.events.split_at(self.next.min(self.events.len()));
        new.iter().chain(old).copied()
    }

    /// Everything from `tick` onward. What the harness's `trace since` serves.
    pub fn since(&self, tick: u64) -> impl Iterator<Item = (u64, E)> + '_ {
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

    fn push(&mut self, tick: u64, event: E) {
        if let Some(capture) = &mut self.capture
            && (capture.accepts)(event) {
            if capture.events.len() < capture.limit {
                capture.events.push((tick, event));
            } else {
                capture.overflowed = true;
            }
        }
        if self.capacity == 0 { return; }
        if self.events.len() < self.capacity {
            self.events.push((tick, event));
            // `next` only means anything once the buffer is full; until then the
            // events are already in order and `split_at` below handles it.
            self.next = 0;
        } else {
            self.events[self.next] = (tick, event);
            self.next = (self.next + 1) % self.capacity;
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
pub struct TraceSink<'a, E = Event> {
    trace: &'a mut Trace<E>,
    tick: u64,
}

impl<E: Copy + fmt::Display> TraceSink<'_, E> {
    /// Records an event at the current tick.
    pub fn emit(&mut self, event: E) {
        self.trace.push(self.tick, event);
    }

    /// Re-borrows, so one tick's sink can be handed to several passes in turn.
    pub fn reborrow(&mut self) -> TraceSink<'_, E> {
        TraceSink { trace: self.trace, tick: self.tick }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_capacity_preserves_history_and_capture() {
        let mut trace = Trace::default();
        trace.set_retention(1).unwrap();
        trace.capture(2, |_| true).unwrap();
        trace.sink(0).emit(Event::Swung);
        for invalid in [MAX_TRACE_EVENTS + 1, usize::MAX] {
            assert!(trace.set_retention(invalid).is_err());
            assert!(trace.capture(invalid, |_| false).is_err());
            assert_eq!(trace.iter().collect::<Vec<_>>(), vec![(0, Event::Swung)]);
            assert_eq!(trace.captured(), Some(&[(0, Event::Swung)][..]));
        }
        trace.sink(1).emit(Event::HitboxOpened);
        assert_eq!(trace.iter().collect::<Vec<_>>(), vec![(1, Event::HitboxOpened)]);
        assert_eq!(trace.captured(), Some(&[(0, Event::Swung), (1, Event::HitboxOpened)][..]));
    }

    #[test]
    fn capture_is_filtered_and_survives_ring_clear_disable_and_overwrite() {
        for retention in [0, 1, 4] {
            let mut trace = Trace::default();
            trace.set_retention(retention).unwrap();
            trace.capture(2, |event| matches!(event, Event::Swung)).unwrap();
            trace.sink(0).emit(Event::Swung);
            for tick in 1..10 { trace.sink(tick).emit(Event::Contacts { count: 1 }); }
            trace.clear();
            trace.sink(10).emit(Event::Swung);
            assert_eq!(trace.captured(), Some(&[(0, Event::Swung), (10, Event::Swung)][..]));
            trace.sink(11).emit(Event::Swung);
            assert!(trace.captured().is_none(), "required evidence cannot silently truncate");
            trace.clear();
            assert!(trace.captured().is_none(), "clearing history cannot bless lost evidence");
        }
    }

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
        trace.sink(0).emit(Event::Refused { count: 64 });
        trace.sink(3).emit(Event::Contacts { count: 2 });
        trace.sink(3).emit(Event::Clamped { count: 1 });

        assert_eq!(trace.render(), "0 refused count=64\n3 contacts count=2\n3 clamped count=1\n");
    }
}
