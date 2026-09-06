//! What a `.ron` scenario file says.
//!
//! Kept deliberately small. Every field here is something an assertion can be
//! written against today; a field that only *describes* an intention is a field
//! that will drift away from what the runner actually checks.

use serde::Deserialize;

/// One scenario: a world, an input stream, a tick budget, and what is expected
/// at the end of it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scenario {
    /// Why this scenario exists and what it would catch. Optional in the type
    /// and required in practice — a scenario nobody can read is a scenario
    /// nobody will update when it starts failing for a good reason.
    #[serde(default)]
    pub(crate) description: String,

    #[serde(default)]
    pub(crate) setup: Setup,

    /// Applied in order, so a later span overwrites an earlier one where they
    /// overlap. Ticks outside every span get no input at all.
    #[serde(default)]
    pub(crate) inputs: Vec<Span>,

    pub(crate) budget: Budget,

    #[serde(default)]
    pub(crate) expect: Expect,
}

/// The world a scenario starts from.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    /// Zero is the useful default, not an edge case: the horde spawns centred
    /// on the origin and so does the player, so any horde at all puts the two
    /// in contact on tick zero — and a prediction about movement then becomes a
    /// prediction about the contact solver, which cannot be made by hand.
    #[serde(default)]
    pub(crate) enemies: usize,

    /// Bodies placed and removed, in order, after the horde grid is laid out.
    ///
    /// **This is what makes a scenario able to say anything precise.** A horde
    /// count puts N bodies in a grid whose positions nobody wrote down, so the
    /// only predictions available were about the player. One body at a stated
    /// place is a prediction anyone can make by hand and check by reading.
    ///
    /// **An ordered list rather than a set of placements plus a set of
    /// removals**, and the order is load-bearing rather than tidy. A slot is
    /// recycled only when something is spawned *after* a despawn, so unordered
    /// setup cannot express the case where a retired name might come back to
    /// life — which is the one failure generational ids exist to prevent, and
    /// the only one that is silent. A first version of this file had exactly
    /// that shape, and a scenario written in it passed with the generation bump
    /// deleted.
    #[serde(default)]
    pub(crate) actions: Vec<Action>,
}

/// One step of scenario setup.
#[derive(Debug, Deserialize)]
pub(crate) enum Action {
    /// Puts a body at a world-space `(x, z)`, on the same terms as
    /// [`Span::dir`]: the simulation's own axes, never the camera's.
    ///
    /// Placements are numbered in the order they appear, across the whole
    /// list, and that number is what [`BodyExpect::nth`] refers to.
    Place((f32, f32)),

    /// Removes a previously placed body, by its placement number.
    ///
    /// The slot it frees is reused by the next `Place`, which is precisely the
    /// case worth writing a scenario about.
    Despawn(usize),
}

/// Hold a direction for a span of ticks.
///
/// **World space, not screen space**, and the difference matters. The game maps
/// screen-right onto the world diagonal `(+X, -Z)/√2`, but that mapping belongs
/// to the camera — it depends on the camera's angle, which is a presentation
/// decision that may yet change. A scenario asserts what the *simulation* does,
/// so it speaks the simulation's own axes and no scenario has to be rewritten
/// if the view ever rotates.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Span {
    /// First tick this input applies to, counting from 0.
    pub(crate) at: u64,
    /// How many ticks to hold it.
    pub(crate) ticks: u64,
    /// World-space `(x, z)`. Normalised on the way in, so `(1, 1)` is a
    /// diagonal at full speed rather than at √2 times it. `(0, 0)` is no input.
    pub(crate) dir: (f32, f32),
}

/// How long the scenario is allowed to run.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Budget {
    /// Exactly this many ticks are run. Not an upper bound — a fixed timestep
    /// makes the count deterministic, so "ran fewer ticks than expected" is a
    /// bug rather than a timing artefact worth tolerating.
    pub(crate) ticks: u64,
}

/// What must hold once the budget is spent.
///
/// Every field is optional; a scenario asserting nothing still checks that it
/// runs to budget without panicking and that it replays identically, which is
/// worth something but is not worth much. Say what you predict.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expect {
    /// Ground-plane position as `(x, z)`.
    #[serde(default)]
    pub(crate) player_pos: Option<Approx2>,
    /// Facing in radians. Yaw 0 faces world `+Z`, positive turns toward `+X`.
    #[serde(default)]
    pub(crate) facing: Option<Approx>,
    #[serde(default)]
    pub(crate) contacts: Option<usize>,
    /// Enemy pairs the crowd solver pushed apart on the final tick.
    #[serde(default)]
    pub(crate) crowd_contacts: Option<usize>,
    #[serde(default)]
    pub(crate) enemy_count: Option<usize>,
    /// Predictions about individual placed bodies.
    #[serde(default)]
    pub(crate) bodies: Vec<BodyExpect>,
    /// A checked-in trace file, relative to the scenario, that the run's own
    /// trace must match exactly.
    ///
    /// For behaviour too broad to enumerate as assertions. A tuning change then
    /// produces a reviewable *diff* rather than a claim — which is the only
    /// form in which "this changed the timing of everything by one tick" is
    /// visible at all.
    ///
    /// Regenerate with `--bless`, and **read the diff before committing it**. A
    /// blessed golden file nobody looked at is a test that has been deleted
    /// without anyone noticing.
    #[serde(default)]
    pub(crate) trace: Option<String>,
}

/// A predicted scalar and how far off it may be.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approx {
    pub(crate) value: f32,
    /// **For float accumulation, never for timing slop.** Under a fixed
    /// timestep the tick count is exact, so a tolerance wide enough to absorb a
    /// one-tick error is not a tolerance — it is the assertion being switched
    /// off. One tick of walking is 0.15 world units.
    pub(crate) tol: f32,
}

/// A predicted ground-plane position and how far off it may be.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approx2 {
    pub(crate) x: f32,
    pub(crate) z: f32,
    /// Euclidean distance, so this is a radius rather than a per-axis slack.
    pub(crate) tol: f32,
}

/// What must hold for one body that [`Setup::bodies`] placed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BodyExpect {
    /// Which placement this is about, by spawn order in [`Setup::bodies`].
    ///
    /// **Deliberately not a dense-array index.** The horde is stored densely
    /// and a despawn swaps the last row into the hole, so a body's index
    /// changes without anything touching that body. The runner holds the real
    /// [`arpg_sim::EntityId`] it got back from each placement and looks it up
    /// by this, which is exactly the indirection the ids exist to provide — a
    /// scenario asserting on index 2 would silently start asserting about a
    /// different body the first time something before it died.
    pub(crate) nth: usize,

    /// Where the body should be. Omit to say nothing about position.
    #[serde(default)]
    pub(crate) pos: Option<Approx2>,

    /// Whether the body should still exist.
    ///
    /// `alive: false` is the assertion that a despawned name stays dead. It is
    /// not the same as saying nothing: a stale id resolving to whichever body
    /// took its row is the exact bug generational ids exist to prevent, and it
    /// reads as success to every other assertion in the file.
    #[serde(default)]
    pub(crate) alive: Option<bool>,
}
