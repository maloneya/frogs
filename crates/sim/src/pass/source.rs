//! Spawn producers with independent cadence, condition, placement and template.
//!
//! A ready source waits for its condition without restarting the countdown.
//! Disabling freezes countdown and emission sequence; it does not cancel queued work.

use core::fmt;

use arpg_core::Report;
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::angle::GOLDEN_ANGLE;
use crate::hash::Fnv;
use crate::pass::spawn::SpawnQueue;
use crate::trace::{Event, TraceSink};
use crate::{SceneId, Template};

/// Source identity, distinct from body IDs. Rows are never reused within a run;
/// retired IDs remain absent, at the cost of storage proportional to historical sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceId(u32);

impl SourceId {
    /// Feeds the complete identity into a deterministic fingerprint.
    pub fn hash_into(self, hash: &mut Fnv) {
        hash.u64(u64::from(self.0));
    }

    /// Parses the source identity syntax emitted by [`fmt::Display`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.strip_prefix('s')?.parse().ok().map(Self)
    }
}

impl fmt::Display for SourceId {
    /// Distinct from `EntityId`'s `#0v2` on purpose: the two appear in the same
    /// trace, and a golden file where they looked alike would hide a mix-up.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}", self.0)
    }
}

/// The gate checked when a source's cadence comes ready.
///
/// Three kinds of question rather than three phrasings of one: what time it is,
/// how many bodies exist, and where the player is standing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum Condition {
    /// Fire on the cadence, always. The default.
    #[default]
    Always,
    /// Fire only while the horde is smaller than this.
    ///
    /// **Maintains a population rather than counting emissions.** A source that
    /// promised twelve bodies would keep its promise into an arena the player
    /// has already cleared; this one refills what was killed and then stops,
    /// which is the behaviour almost every use of a count is reaching for.
    FewerThan(usize),
    /// Fire only while the player is within this distance of the placement's
    /// anchor.
    ///
    /// Measured from the source position in [`SourceSpec::pos`],
    /// so a source does not carry a second position that could disagree with
    /// the first.
    PlayerWithin(f32),
}

impl Condition {
    fn met(self, player: Vec2, bodies: usize, anchor: Vec2) -> bool {
        match self {
            Self::Always => true,
            Self::FewerThan(n) => bodies < n,
            // Squared, so proximity costs no `sqrt` on a test that runs for
            // every source on every tick.
            Self::PlayerWithin(radius) => (player - anchor).length_squared() <= radius * radius,
        }
    }

    fn hash_into(self, h: &mut Fnv) {
        // The discriminant as well as the payload: `FewerThan(0)` and
        // `Always` are different sources with the same number in them.
        match self {
            Self::Always => h.usize(0),
            Self::FewerThan(n) => {
                h.usize(1);
                h.usize(n);
            }
            Self::PlayerWithin(radius) => {
                h.usize(2);
                h.f32(radius);
            }
        }
    }
}

/// Where a source puts what it makes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Placement {
    /// Always the same ground-plane point.
    At(Vec2),
    /// Golden-angle ring placement, spreading successive emissions instead of stacking them.
    Ring {
        /// Ground-plane centre.
        centre: Vec2,
        /// Distance from the centre. Not asserted against the body radius here:
        /// a ring narrower than a body is legal and merely produces a pile the
        /// solver sorts out, which is exactly what `At` does on purpose.
        radius: f32,
    },
}

impl Placement {
    /// Where the `nth` body from this source goes.
    fn point(self, nth: u32) -> Vec2 {
        match self {
            Self::At(p) => p,
            Self::Ring { centre, radius } => {
                let angle = nth as f32 * GOLDEN_ANGLE;
                centre + Vec2::new(angle.cos(), angle.sin()) * radius
            }
        }
    }

    /// The one point that stands for this placement, which is what a proximity
    /// condition measures against.
    fn anchor(self) -> Vec2 {
        match self {
            Self::At(p) => p,
            Self::Ring { centre, .. } => centre,
        }
    }

    fn hash_into(self, h: &mut Fnv) {
        match self {
            Self::At(p) => {
                h.usize(0);
                h.f32(p.x);
                h.f32(p.y);
            }
            Self::Ring { centre, radius } => {
                h.usize(1);
                h.f32(centre.x);
                h.f32(centre.y);
                h.f32(radius);
            }
        }
    }
}

/// Live source state, also used directly for observation and scenario assertions.
/// A returned value is a copy; changing it cannot mutate a running source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceState {
    /// Whether the source may advance its cadence and request spawns.
    pub enabled: bool,
    /// Enabled ticks to spend waiting before the source is ready to fire.
    pub countdown: u32,
    /// Accepted emissions so far; determines the next point of a ring placement.
    pub emitted: u32,
}

impl SourceState {
    fn hash(&self, h: &mut Fnv) {
        let Self { enabled, countdown, emitted } = self;
        h.usize(usize::from(*enabled));
        h.usize(*countdown as usize);
        h.usize(*emitted as usize);
    }

    fn report(&self, out: &mut Report) {
        let Self { enabled, countdown, emitted } = self;
        out.bool("enabled", *enabled);
        out.int("countdown", u64::from(*countdown));
        out.int("emitted", u64::from(*emitted));
    }
}

/// One thing that asks for spawns.
///
/// Deserialised through [`SourceSpec`], which is the only written form of one
/// and the only `Deserialize` path to this type.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "SourceSpec")]
pub struct Source {
    placement: Placement,
    what: Template,
    /// Ticks between emissions. Never zero — see [`SourceSpec::every`].
    every: u32,
    condition: Condition,
    state: SourceState,
}

impl Source {
    fn hash_into(&self, h: &mut Fnv) {
        // Exhaustive, as every hash in this crate is: a new axis stops this
        // compiling until someone has decided it is state.
        let Self { placement, what, every, condition, state } = self;

        placement.hash_into(h);
        what.hash_into(h);
        condition.hash_into(h);
        h.usize(*every as usize);
        state.hash(h);
    }
}

/// Authored source specification, shared by files and direct callers.
/// Conversion to [`Source`] validates admission; decoding uses that conversion.
/// Zero radius is a point placement; zero cadence normalizes to one tick.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    /// Initially enabled by default. False creates a ready but dormant source.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Ground-plane `(x, z)`: the point bodies appear at, or the centre of the
    /// ring they appear around.
    pub pos: (f32, f32),
    /// Radius of the ring around `pos`. Zero — the default — means the one
    /// point, which stacks successive bodies on each other on purpose.
    #[serde(default)]
    pub radius: f32,
    /// Ticks between emissions. A source starts *ready*, so the first lands on
    /// the first tick its gate is open, not one cadence later.
    #[serde(default = "one")]
    pub every: u32,
    /// The gate checked when the cadence comes ready.
    #[serde(default)]
    pub when: Condition,
    /// What it makes.
    #[serde(default)]
    pub what: Template,
}

/// A cadence of one is "every tick", which is the useful default: a gated
/// source then reacts as fast as its gate changes.
fn one() -> u32 {
    1
}

fn enabled_by_default() -> bool {
    true
}

impl TryFrom<SourceSpec> for Source {
    type Error = &'static str;

    fn try_from(spec: SourceSpec) -> Result<Self, Self::Error> {
        let SourceSpec { pos, radius, every, when, what, enabled } = spec;
        let centre = Vec2::from(pos);
        if !centre.is_finite() || !radius.is_finite() || radius < 0.0
            || !(centre.abs() + Vec2::splat(radius)).is_finite()
        {
            return Err("source position/radius must be finite; radius must be nonnegative");
        }
        if !what.valid_at(centre.abs() + Vec2::splat(radius)) {
            return Err("invalid source template or static placement extent");
        }
        if let Condition::PlayerWithin(distance) = when
            && (!distance.is_finite() || distance < 0.0)
        {
            return Err("source proximity must be finite and nonnegative");
        }
        Ok(Self {
            placement: if radius == 0.0 { Placement::At(centre) }
                else { Placement::Ring { centre, radius } },
            what,
            every: every.max(1),
            condition: when,
            state: SourceState { enabled, countdown: 0, emitted: 0 },
        })
    }
}

/// A source carries its lifetime owner into every emission.
struct OwnedSource {
    source: Source,
    owner: Option<SceneId>,
}

/// Everything that asks for spawns.
///
/// **Never compacted.** A removed source leaves a hole, so an index is a
/// permanent name and [`SourceId`] needs no generation. The cost is iterating
/// the holes, which is paid at the scale of tens rather than of the horde.
#[derive(Default)]
pub(crate) struct Sources {
    rows: Vec<Option<OwnedSource>>,
}

impl Sources {
    pub(crate) fn state(&self, id: SourceId) -> Option<SourceState> {
        self.rows.get(id.0 as usize)?.as_ref().map(|row| row.source.state)
    }

    /// A repeated setting succeeds without an event. A retired name is refused.
    /// The signature has no queue access: accepted requests cannot be cancelled.
    pub(crate) fn set_enabled(
        &mut self, id: SourceId, enabled: bool, mut trace: TraceSink<'_>,
    ) -> bool {
        let Some(Some(row)) = self.rows.get_mut(id.0 as usize) else {
            return false;
        };
        if row.source.state.enabled != enabled {
            row.source.state.enabled = enabled;
            trace.emit(Event::SourceEnablementChanged { id, enabled });
        }
        true
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { rows } = self;
        for (index, row) in rows.iter().enumerate() {
            if let Some(row) = row {
                out.object(&SourceId(index as u32).to_string(), |out| row.source.state.report(out));
            }
        }
    }

    pub(crate) fn can_add(&self, count: usize) -> bool {
        self.rows.len().checked_add(count).is_some_and(|end| end <= u32::MAX as usize)
    }

    pub(crate) fn add(&mut self, source: Source) -> SourceId {
        self.add_owned(source, None)
    }

    pub(crate) fn add_owned(&mut self, source: Source, owner: Option<SceneId>) -> SourceId {
        assert!(self.can_add(1), "source identities exhausted; ids must never wrap");
        let id = SourceId(self.rows.len() as u32);
        self.rows.push(Some(OwnedSource { source, owner }));
        id
    }

    /// Removes one. `false` if it was already gone, which is the ordinary case
    /// for anything holding a name across the moment its source died.
    pub(crate) fn remove(&mut self, id: SourceId) -> bool {
        match self.rows.get_mut(id.0 as usize) {
            Some(row @ Some(_)) => {
                *row = None;
                true
            }
            _ => false,
        }
    }

    /// How many sources are live. Not `rows.len()`, which counts the dead.
    pub(crate) fn len(&self) -> usize {
        self.rows.iter().flatten().count()
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { rows } = self;

        // Length as well as contents, and the holes as well as the live rows:
        // two worlds with one live source are different worlds if one of them
        // has a dead row before it, because the next source added takes a
        // different name in each.
        h.usize(rows.len());
        for row in rows {
            match row {
                Some(source) => {
                    h.usize(1);
                    let OwnedSource { source, owner } = source;
                    source.hash_into(h);
                    h.usize(usize::from(owner.is_some()));
                    if let Some(owner) = owner {
                        owner.hash_into(h);
                    }
                }
                None => h.usize(0),
            }
        }
    }
}

/// Queues ready sources before the spawn drain, so accepted bodies can act this tick.
pub(crate) fn trigger(
    sources: &mut Sources,
    queue: &mut SpawnQueue,
    player: Vec2,
    bodies: usize,
    mut trace: TraceSink<'_>,
) {
    for (row, source) in sources.rows.iter_mut().enumerate() {
        let Some(OwnedSource { source, owner }) = source else {
            continue;
        };

        if !source.state.enabled {
            continue;
        }

        // Cadence first, condition second. A ready source held back by its
        // condition stays ready — see the module docs.
        if source.state.countdown > 0 {
            source.state.countdown -= 1;
            continue;
        }

        if !source.condition.met(player, bodies, source.placement.anchor()) {
            continue;
        }

        let id = SourceId(row as u32);
        let at = source.placement.point(source.state.emitted);

        // **A refused request still spends the cadence.** The queue counts the
        // refusal and the drain reports it, so nothing is lost; retrying on the
        // next tick instead would turn a saturated world into a source that
        // asks sixty times a second forever.
        if queue.push_from(at, source.what, *owner, Some(id)).is_some() {
            source.state.emitted += 1;
            // Per emission rather than summarised, and it is the one event here
            // that names *why* a body exists. `placed` says a body appeared;
            // this says which source asked for it, which is the only way to
            // tell two sources apart in a trace.
            trace.emit(Event::Fired { source: id });
        }

        // `every - 1`, not `every`: this tick is the first of the interval. At
        // `every = 1` that is zero, so the source is ready again immediately.
        source.state.countdown = source.every - 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Accumulator, Dt, World};
    use arpg_core::Intent;

    #[test]
    fn disabling_does_not_cancel_an_accepted_source_emission() {
        let mut world = World::empty();
        let id = world.add_source(
            Source::try_from(SourceSpec { pos: (20.0, 0.0), radius: 0.0, every: 2,
                when: Condition::Always, what: Template::BODY, enabled: true }).unwrap(),
        );
        // Exercise the interval between decision and drain directly: the
        // enablement setter has no authority over an already accepted request.
        trigger(&mut world.sources, &mut world.queue, Vec2::ZERO, 0, world.trace.sink(0));
        assert_eq!(world.queue.len(), 1);
        assert!(world.set_source_enabled(id, false));
        for dt in Accumulator::default().pending(Dt::SECS) {
            world.step(dt, Intent::new(arpg_core::MoveDir::NONE, arpg_core::AimDir::FORWARD, false));
        }
        assert_eq!(world.enemy_count(), 1);
        assert_eq!(world.source_state(id), Some(SourceState {
            enabled: false, countdown: 1, emitted: 1,
        }));
    }
}
