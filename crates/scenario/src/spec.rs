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
    #[serde(default)]
    pub(crate) enemy_count: Option<usize>,
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
