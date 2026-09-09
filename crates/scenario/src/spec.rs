//! What a `.ron` scenario file says.
//!
//! Kept deliberately small. Every field here is something an assertion can be
//! written against today; a field that only *describes* an intention is a field
//! that will drift away from what the runner actually checks.

use arpg_sim::{AttackPhase, AttackProfile, Impulse, RecoveryTicks, Source, Template};
use serde::Deserialize;

/// One scenario: a world, an input stream, a tick budget, and what is expected
/// at checkpoints and at the end of it.
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

    /// Lifecycle operations before their named tick, in file order.
    #[serde(default)]
    pub(crate) scenes: Vec<SceneAt>,

    /// Applied in order, so a later span overwrites an earlier one where they
    /// overlap. Ticks outside every span get no input at all.
    #[serde(default)]
    pub(crate) inputs: Vec<Span>,

    /// Ticks on which the attack button is pressed.
    ///
    /// A list of moments rather than spans, because a swing is an **edge**: it
    /// is asked for once and then runs on its own schedule. Writing it as a
    /// span would invite `ticks: 20` and quietly mean something the simulation
    /// does not do — a press during a swing is dropped, not held.
    #[serde(default)]
    pub(crate) attacks: Vec<u64>,

    /// Validated recovery edits, applied before the named tick in file order.
    #[serde(default)]
    pub(crate) attack_recovery: Vec<RecoveryAt>,

    /// Authored attack selections, applied before the named tick.
    #[serde(default)]
    pub(crate) attack_profiles: Vec<AttackProfileAt>,

    /// External momentum changes, applied before the named tick.
    #[serde(default)]
    pub(crate) impulses: Vec<ImpulseAt>,

    /// Things asked for *while the scenario runs*, each at a stated tick.
    ///
    /// **Distinct from [`Setup::actions`], and the distinction is the whole
    /// point of the spawn queue.** A setup placement happens before tick zero,
    /// with the schedule stopped; this goes through the same door a trigger
    /// inside the simulation will use, so what a scenario exercises is the real
    /// path rather than a test-only shortcut.
    ///
    /// A request made between ticks is granted by the first pass of the next
    /// tick, so a spawn `at: 10` is a body from the start of tick 10 and does
    /// not exist at the end of tick 9.
    ///
    /// Bodies that appear this way continue the placement numbering that
    /// [`Setup::actions`] starts, in the order they were granted, so
    /// [`BodyExpect::nth`] can name one.
    #[serde(default)]
    pub(crate) spawns: Vec<Spawn>,

    /// Sources removed partway through, each at a stated tick.
    #[serde(default)]
    pub(crate) remove_sources: Vec<SourceRemoval>,

    /// State assertions immediately after the named zero-based tick completes.
    /// Input order is arbitrary; multiple checkpoints may inspect the same tick.
    #[serde(default)]
    pub(crate) checkpoints: Vec<Checkpoint>,

    pub(crate) budget: Budget,

    #[serde(default)]
    pub(crate) expect: Expect,
}

/// A tuning command uses sim's value type, including its deserialization guard.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryAt {
    pub(crate) at: u64,
    pub(crate) recovery: RecoveryTicks,
}

/// The same authored attack identity selected by the in-game panel.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackProfileAt {
    pub(crate) at: u64,
    pub(crate) profile: AttackProfile,
}

/// A point-in-time assertion using exactly the final state's vocabulary.
/// Golden traces describe the whole run and stay in the final `expect`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    /// Runs after tick `at`, when `World::tick()` is `at + 1`.
    /// Must be below the scenario's tick budget; there is no implicit setup tick.
    pub(crate) at: u64,
    pub(crate) expect: Expect,
}

/// The world a scenario starts from.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    /// Real authored content, instantiated through the same door as the game.
    #[serde(default)]
    pub(crate) scenes: Vec<SceneRef>,
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

    /// Things that ask for spawns while the scenario runs.
    ///
    /// Added before tick zero and numbered in the order they appear, which is
    /// what [`SourceRemoval::source`] refers to. A source is not a body and
    /// takes no placement number.
    #[serde(default)]
    pub(crate) sources: Vec<Source>,
}

/// A source removed partway through the run.
///
/// **The other half of the flow being controllable.** A source that can only be
/// added describes a level that starts; one that can be removed describes a
/// level that can be finished.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRemoval {
    /// The tick it is removed on, before that tick runs. A source removed at
    /// `at` does not fire on `at`.
    pub(crate) at: u64,
    /// Which source, by its position in [`Setup::sources`].
    pub(crate) source: usize,
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

    /// Makes a previously placed body chase the player, by its placement
    /// number.
    ///
    /// **Granted separately from placing it, on purpose.** A body is a body;
    /// what makes it an enemy is the list of behaviours attached to it. Writing
    /// that as `Place` then `Seek` keeps the scenario language honest about the
    /// storage underneath, where seeking is a membership set and not a field —
    /// and it means the next behaviour is a new action rather than a new kind
    /// of placement.
    Seek(usize),
}

/// One thing asked for at a stated tick.
///
/// What the body is granted once it exists is a *template*, and it is the
/// simulation's own [`arpg_sim::Template`] rather than a copy of its fields.
/// That is what keeps "two kinds of enemy" a difference in a description rather
/// than a difference in code, and keeps this file from growing a field every
/// time a behaviour is added.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Spawn {
    /// The tick the request is made on, counting from 0. The body exists for
    /// the whole of that tick.
    pub(crate) at: u64,
    /// World-space `(x, z)`, on the same terms as [`Action::Place`].
    pub(crate) pos: (f32, f32),
    /// What the new body is granted. Spelled `what: (seeks: true)`.
    #[serde(default)]
    pub(crate) what: Template,
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
    /// Mean time in World::step only; excludes setup, hashing and assertions.
    #[serde(default)]
    pub(crate) max_mean_step_micros: Option<f64>,
}

/// State assertions shared by final expectations and checkpoints.
///
/// Every field is optional; a scenario asserting nothing still checks that it
/// runs to budget without panicking and that it replays identically, which is
/// worth something but is not worth much. Say what you predict.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expect {
    #[serde(default)]
    pub(crate) attack_phase: Option<AttackPhase>,
    #[serde(default)]
    pub(crate) swing_tick: Option<u32>,
    #[serde(default)]
    pub(crate) recovery_ticks: Option<u32>,
    /// Zero when no swing is in flight.
    #[serde(default)]
    pub(crate) swing_recovery_ticks: Option<u32>,
    /// Authored attack selected for the next swing.
    #[serde(default)]
    pub(crate) attack_profile: Option<AttackProfile>,
    /// Authored attack captured by the swing in flight.
    #[serde(default)]
    pub(crate) swing_profile: Option<AttackProfile>,
    /// Ground-plane position as `(x, z)`.
    #[serde(default)]
    pub(crate) player_pos: Option<Approx2>,
    #[serde(default)]
    pub(crate) player_velocity: Option<Approx2>,
    /// Facing in radians. Yaw 0 faces world `+Z`, positive turns toward `+X`.
    #[serde(default)]
    pub(crate) facing: Option<Approx>,
    #[serde(default)]
    pub(crate) contacts: Option<usize>,
    /// Enemy pairs the crowd solver pushed apart on the observed tick.
    #[serde(default)]
    pub(crate) crowd_contacts: Option<usize>,
    /// How many bodies the last swing struck.
    #[serde(default)]
    pub(crate) struck: Option<usize>,
    /// Whether the hitbox exists on the final tick.
    #[serde(default)]
    pub(crate) hitbox: Option<bool>,
    #[serde(default)]
    pub(crate) enemy_count: Option<usize>,
    /// How many bodies chase the player. The cheapest way to assert that two
    /// bodies made from different templates were granted different behaviours.
    #[serde(default)]
    pub(crate) seekers: Option<usize>,
    /// How many sources are still live at the end.
    #[serde(default)]
    pub(crate) sources: Option<usize>,
    #[serde(default)]
    pub(crate) scene_count: Option<usize>,
    /// Predictions about individual bodies placed by [`Setup::actions`].
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

impl Approx2 {
    /// How far `got` misses by, or `None` if it is within tolerance.
    ///
    /// **One copy of this comparison, deliberately.** `run::check_finite`'s doc
    /// names `(got - want).length() > tolerance` as *the* shape every positional
    /// assertion has — and the reason a NaN slips through all of them. That
    /// argument is about one shape; it stops being true the moment there are two
    /// that can drift. It is also the only place the "`y` is world Z" convention
    /// is applied on the way in.
    pub(crate) fn off_by(&self, got: glam::Vec3) -> Option<f32> {
        let off = (glam::Vec2::new(got.x, got.z) - glam::Vec2::new(self.x, self.z)).length();
        (off > self.tol).then_some(off)
    }

    /// The prediction, as it appears in a failure.
    pub(crate) fn expected(&self) -> String {
        format!("({:.4}, {:.4}) +/- {:.4}", self.x, self.z, self.tol)
    }
}

/// What must hold for one body that [`Setup::bodies`] placed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BodyExpect {
    /// Which placement this is about, by spawn order in [`Setup::actions`].
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
    #[serde(default)]
    pub(crate) velocity: Option<Approx2>,

    /// Remaining hits before defeat. Only live enemy bodies have health.
    #[serde(default)]
    pub(crate) health: Option<u8>,

    /// Whether the body should be chasing the player.
    ///
    /// `seeking: false` on a body that was never granted it is the control in
    /// the pair: a pass that moved everything would satisfy every assertion
    /// about the chaser and only fail here.
    #[serde(default)]
    pub(crate) seeking: Option<bool>,

    /// Whether the body should still exist.
    ///
    /// `alive: false` is the assertion that a despawned name stays dead. It is
    /// not the same as saying nothing: a stale id resolving to whichever body
    /// took its row is the exact bug generational ids exist to prevent, and it
    /// reads as success to every other assertion in the file.
    #[serde(default)]
    pub(crate) alive: Option<bool>,
}

/// A scenario's reference to a body; identity itself is resolved by the runner.
#[derive(Debug, Deserialize)]
pub(crate) enum Target {
    Player,
    Placed(usize),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImpulseAt {
    pub(crate) at: u64,
    pub(crate) target: Target,
    /// Uses sim's validated vocabulary, not a second definition of momentum.
    pub(crate) value: Impulse,
}

/// Test orchestration refers to the simulation's content type directly.
#[derive(Debug, Deserialize)]
pub(crate) enum SceneRef {
    Inline(arpg_sim::Scene),
    File(std::path::PathBuf),
}

impl SceneRef {
    pub(crate) fn resolve(&mut self, base: &std::path::Path) -> Result<(), scenario::LoadError> {
        if let Self::File(path) = self {
            *self = Self::Inline(scenario::load_scene(&base.join(path))?);
        }
        Ok(())
    }

    pub(crate) fn content(&self) -> &arpg_sim::Scene {
        match self {
            Self::Inline(scene) => scene,
            Self::File(_) => panic!("scene references must resolve before validation and replay"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SceneAt {
    pub(crate) at: u64,
    pub(crate) action: SceneAction,
}

/// Indices refer to load order, including setup, never to a content name.
#[derive(Debug, Deserialize)]
pub(crate) enum SceneAction {
    Load(SceneRef),
    Evict(usize),
}
