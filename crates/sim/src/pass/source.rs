//! Who asks for a spawn, and when.
//!
//! ## A source is not a kind of body
//!
//! It is the thing that *makes* bodies, and hanging it off one of its own
//! products inverts the layering: it would break the moment the thing being
//! made is not a body, and it would make "can this be killed" and "what is
//! this" the same question. So a source lives in its own list, is named by its
//! own id, and holds a position only because it needs somewhere to put things.
//!
//! What binds it to the rest of the simulation is a *request*, not a type: this
//! module pushes onto [`crate::pass::spawn::SpawnQueue`] and can do nothing
//! else. The pass signature says so — it is handed no storage, so it physically
//! cannot spawn, only ask. Every other asker (the harness, a scenario, a body
//! that splits when it dies) is equal to it at that seam.
//!
//! ## Flexibility is in the axes, not in an open set of conditions
//!
//! A source is four independent choices, each small:
//!
//! - **cadence** — [`Source::every`], in ticks;
//! - **condition** — [`Condition`], the gate checked when the cadence is ready;
//! - **placement** — [`Placement`], where the body goes;
//! - **template** — [`crate::Template`], what it is when it gets there.
//!
//! Four axes multiply; a single `Trigger` enum covering every useful
//! combination would have to add a variant per pairing. The condition set stays
//! deliberately short because *that* is the axis a new game requirement most
//! often wants, and each variant is a different kind of question — time,
//! population, proximity — rather than a different phrasing of one.
//!
//! Data rather than a closure, for the reasons a [`crate::Template`] is: the
//! determinism hash destructures exhaustively, scenarios deserialise from RON,
//! and a replay has to reproduce the decision.
//!
//! ## Ready-and-waiting
//!
//! The cadence and the condition are checked in that order, and a source whose
//! cadence is ready but whose condition is shut **stays** ready. It fires on the
//! first tick the condition opens rather than on the next multiple of the
//! cadence after it. That is the behaviour a room full of enemies wants: they
//! appear when the player walks in, not up to `every` ticks later.

use core::fmt;

use glam::Vec2;
use serde::Deserialize;

use crate::angle::GOLDEN_ANGLE;
use crate::hash::Fnv;
use crate::pass::spawn::SpawnQueue;
use crate::trace::{Event, TraceSink};
use crate::{SceneId, Template};

/// The name of one source.
///
/// A bare index, with **no generation**, and that is safe here for a reason
/// that does not hold for bodies: the list is never compacted and a row is
/// never reused, so an index names the same source for the life of the world.
/// A removed source's id resolves to nothing, forever.
///
/// The cost is a row per source ever created. That is the right trade at the
/// scale sources exist at — tens, over a whole session, against a horde of
/// thousands — and it is what `EntityId`'s generation is paying for on the
/// horde's behalf instead.
///
/// A separate type from [`crate::EntityId`] rather than a reuse of it, because
/// the two index different lists: an id that resolved in both would be a
/// confusion the compiler could otherwise not see.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceId(u32);

impl SourceId {
    pub(crate) fn hash(self, hash: &mut Fnv) {
        hash.u64(u64::from(self.0));
    }

    /// Reads a name back from the text [`fmt::Display`] wrote.
    ///
    /// **A door that `EntityId` deliberately does not have**, and the reason
    /// the two differ is the generation. Forging an `EntityId` would let a
    /// caller name a body that died and get the one that took its row; a
    /// `SourceId` is a bare index into a list that is never compacted, so a
    /// forged one either names the source it says or names nothing at all.
    ///
    /// It exists because the harness is a text protocol: an agent reads
    /// `fired source=s3` out of a trace and hands `s3` straight back. Accepting
    /// only the form `Display` produces is what keeps that a round trip rather
    /// than two conventions that can drift.
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
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
    /// Measured from where the source puts things — see [`Placement::anchor`] —
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
pub enum Placement {
    /// Always the same ground-plane point.
    At(Vec2),
    /// Spread around a circle, one step of the golden angle per emission.
    ///
    /// **The reason this exists rather than being a nicety.** Bodies stacked on
    /// one point are coincident, and the separation solver's whole job is then
    /// to blow them apart — a source on a fixed point with a short cadence
    /// builds a pile and then explodes it. Stepping the angle means successive
    /// bodies never land on each other and never fall into a repeating pattern;
    /// see [`GOLDEN_ANGLE`], which the coincident-body tiebreak uses for the
    /// same reason.
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
    /// The placement a centre and a radius describe.
    ///
    /// **A radius of zero is [`Placement::At`], not a ring of nothing.** A
    /// `Ring` of radius zero would put every body on the centre and step an
    /// angle that changes nothing, so the two spellings are not interchangeable
    /// and a copy of this rule that drifted would be a placement that silently
    /// stopped spreading.
    ///
    /// It had two call sites the moment it existed — the scenario runner and
    /// the harness, in different crates — and now has one, because both of them
    /// describe a source as a [`SourceSpec`] and this is what the conversion
    /// calls. That is the shape worth keeping: a rule reachable only through the
    /// single door every description passes through cannot be half-applied.
    #[must_use]
    pub fn around(centre: Vec2, radius: f32) -> Self {
        if radius > 0.0 {
            Self::Ring { centre, radius }
        } else {
            Self::At(centre)
        }
    }

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

/// One thing that asks for spawns.
///
/// Deserialised through [`SourceSpec`], which is the only written form of one
/// and the only `Deserialize` path to this type.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(from = "SourceSpec")]
pub struct Source {
    placement: Placement,
    what: Template,
    /// Ticks between emissions. Never zero — see [`Source::every`].
    every: u32,
    condition: Condition,
    /// Ticks left before the cadence is ready. Zero means ready *now*, and a
    /// ready source that the condition holds back stays ready.
    countdown: u32,
    /// How many bodies this source has made. State, not a statistic: it is what
    /// steps a ring placement around, so two runs that disagree on it put
    /// bodies in different places.
    emitted: u32,
}

impl Source {
    /// A source that fires every tick the condition allows.
    ///
    /// **Starts ready**, so it fires on the first tick it is evaluated rather
    /// than after one cadence. Otherwise the first body of every level would
    /// arrive at a time nobody wrote down.
    #[must_use]
    pub const fn new(placement: Placement, what: Template) -> Self {
        Self {
            placement,
            what,
            every: 1,
            condition: Condition::Always,
            countdown: 0,
            emitted: 0,
        }
    }

    /// Sets the cadence, in ticks. Clamped to at least 1, because a cadence of
    /// zero is not "as fast as possible" — it is a division by nothing, and the
    /// smallest real interval is one tick.
    #[must_use]
    pub const fn every(mut self, ticks: u32) -> Self {
        self.every = if ticks == 0 { 1 } else { ticks };
        self
    }

    /// Sets the gate checked when the cadence comes ready.
    #[must_use]
    pub const fn when(mut self, condition: Condition) -> Self {
        self.condition = condition;
        self
    }

    fn hash_into(&self, h: &mut Fnv) {
        // Exhaustive, as every hash in this crate is: a new axis stops this
        // compiling until someone has decided it is state.
        let Self { placement, what, every, condition, countdown, emitted } = self;

        placement.hash_into(h);
        what.hash_into(h);
        condition.hash_into(h);
        h.usize(*every as usize);
        h.usize(*countdown as usize);
        h.usize(*emitted as usize);
    }
}

/// The written form of a source: the four axes, flat, as a scenario or a level
/// file spells them.
///
/// **This is the one definition of what a source looks like from outside.** It
/// replaced a copy in the scenario crate's spec and a third in the harness
/// parser, each of which had to grow a field per axis, and neither of which
/// could express an axis it had not been taught about.
///
/// The argument for those copies was that deriving `Deserialize` on the
/// simulation's own types makes a `.ron` file a consumer of its internals, so a
/// rename in `sim` changes the scenario language underneath the gate. Half of
/// that is right, and it is the harmless half: a rename here breaks every
/// scenario naming the old field **loudly**, at parse time, on the next run of
/// the runner. What the copies hid was the opposite failure — an axis added to
/// [`Source`] and reachable from no `.ron` file at all, which nothing anywhere
/// reports. So the field names below *are* the language, and renaming one is a
/// change to it; that obligation belongs here, where somebody about to rename
/// one is already looking, rather than in a duplicate in another crate.
///
/// **Every axis goes through the validating constructor for it.** Because this
/// is the only way to deserialise a [`Source`], `radius: 0.0` becomes
/// [`Placement::At`] and `every: 0` becomes a cadence of one, with no caller
/// able to reach a `Source` that skipped either rule.
///
/// The fields are public so the harness can fill one in field by field. That is
/// deliberate rather than lax: a new axis then stops the harness compiling until
/// somebody has decided which flag fills it, which is the check a text protocol
/// otherwise has no way to get.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
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

impl From<SourceSpec> for Source {
    fn from(spec: SourceSpec) -> Self {
        let SourceSpec { pos, radius, every, when, what } = spec;

        Source::new(Placement::around(Vec2::new(pos.0, pos.1), radius), what)
            .every(every)
            .when(when)
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
                        owner.hash(h);
                    }
                }
                None => h.usize(0),
            }
        }
    }
}

/// Decides which sources fire this tick, and asks.
///
/// **Handed no storage, and that is the pass contract doing the load-bearing
/// work here.** This is the code that decides new bodies exist, and it cannot
/// create one: it reads two facts about the world and writes to the queue. A
/// version of this taking `&mut World` would compile just as well and would put
/// a structural change back in the middle of the schedule, which is exactly
/// what `pass::spawn` exists to prevent.
///
/// Runs immediately before [`crate::pass::spawn::drain`], so a source that
/// fires on tick N produces a body that lives the whole of tick N. The two
/// could be one pass; they are not, because "decide" and "perform" have
/// different privileges and the split is what lets the type system say so.
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

        // Cadence first, condition second. A ready source held back by its
        // condition stays ready — see the module docs.
        if source.countdown > 0 {
            source.countdown -= 1;
            continue;
        }

        if !source.condition.met(player, bodies, source.placement.anchor()) {
            continue;
        }

        let id = SourceId(row as u32);
        let at = source.placement.point(source.emitted);

        // **A refused request still spends the cadence.** The queue counts the
        // refusal and the drain reports it, so nothing is lost; retrying on the
        // next tick instead would turn a saturated world into a source that
        // asks sixty times a second forever.
        if queue.push_owned(at, source.what, *owner) {
            source.emitted += 1;
            // Per emission rather than summarised, and it is the one event here
            // that names *why* a body exists. `placed` says a body appeared;
            // this says which source asked for it, which is the only way to
            // tell two sources apart in a trace.
            trace.emit(Event::Fired { source: id });
        }

        // `every - 1`, not `every`: this tick is the first of the interval. At
        // `every = 1` that is zero, so the source is ready again immediately.
        source.countdown = source.every - 1;
    }
}
