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

    /// Explicit aim changes, retained until another change. When empty, the
    /// keyboard driver aims along movement and retains its last nonzero direction.
    #[serde(default)]
    pub(crate) aims: Vec<AimAt>,

    /// Ticks on which the attack button is pressed.
    ///
    /// A list of moments rather than spans, because a swing is an **edge**: it
    /// is asked for once and then runs on its own schedule. Writing it as a
    /// span would invite `ticks: 20` and quietly mean something the simulation
    /// does not do — a press during a swing is dropped, not held.
    #[serde(default)]
    pub(crate) attacks: Vec<u64>,

    /// Half-open spans during which attack is held, independently of press edges.
    #[serde(default)]
    pub(crate) channels: Vec<ChannelSpan>,

    /// Ticks on which the interaction button is pressed.
    #[serde(default)]
    pub(crate) interactions: Vec<u64>,

    /// Validated recovery edits, applied before the named tick in file order.
    #[serde(default)]
    pub(crate) attack_recovery: Vec<RecoveryAt>,

    /// Authored attack selections, applied before the named tick.
    #[serde(default)]
    pub(crate) attack_profiles: Vec<AttackProfileAt>,

    /// Complete validated tuning edits before the named tick.
    #[serde(default)]
    pub(crate) tuning: Vec<TuningAt>,

    /// External momentum changes, applied before the named tick.
    #[serde(default)]
    pub(crate) impulses: Vec<ImpulseAt>,

    /// Queued spawns before their named tick: `at: 10` exists during tick 10,
    /// not at the end of tick 9. Granted bodies continue [`BodyExpect::nth`] numbering.
    #[serde(default)]
    pub(crate) spawns: Vec<Spawn>,

    /// Sources removed partway through, each at a stated tick.
    #[serde(default)]
    pub(crate) remove_sources: Vec<SourceRemoval>,

    /// Enablement changes before the named tick, in authored order.
    #[serde(default)]
    pub(crate) source_switches: Vec<SourceSwitch>,
    /// Removes an individually resolved body before the named tick.
    #[serde(default)]
    pub(crate) despawns: Vec<DespawnAt>,

    /// State assertions immediately after the named zero-based tick completes.
    /// Input order is arbitrary; multiple checkpoints may inspect the same tick.
    #[serde(default)]
    pub(crate) checkpoints: Vec<Checkpoint>,

    pub(crate) budget: Budget,

    #[serde(default)]
    pub(crate) expect: Expect,
}

impl Scenario {
    /// The shared order for execution and authored instance-index validation.
    /// Stable sorting preserves file order for operations on the same tick.
    pub(crate) fn scene_commands(&self) -> Vec<&SceneAt> {
        let mut commands: Vec<_> = self.scenes.iter().collect();
        commands.sort_by_key(|command| command.at);
        commands
    }
}

/// A source number refers to setup.sources, just as source removal does.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceSwitch {
    /// Optional scene load index; absent selects setup.sources.
    #[serde(default)]
    pub(crate) scene: Option<usize>,
    pub(crate) at: u64,
    pub(crate) source: usize,
    pub(crate) enabled: bool,
}

/// Exact live state, using the engine's own type rather than a mirror schema.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceExpect {
    /// Optional scene load index; absent selects setup.sources.
    #[serde(default)]
    pub(crate) scene: Option<usize>,
    pub(crate) source: usize,
    /// None asserts that a previously installed source has been removed.
    pub(crate) state: Option<arpg_sim::SourceState>,
}

/// A tuning command uses sim's value type, including its deserialization guard.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryAt {
    pub(crate) at: u64,
    pub(crate) recovery: RecoveryTicks,
}

/// A bounded hold; validated against the scenario budget before setup.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChannelSpan {
    pub(crate) at: u64,
    pub(crate) ticks: u64,
}

/// The same authored attack identity used by gameplay input.
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
    /// Runs after tick `at`, when `arpg_game::Game::tick()` is `at + 1`.
    /// Must be below the scenario's tick budget; there is no implicit setup tick.
    pub(crate) at: u64,
    pub(crate) expect: Expect,
}

/// The world a scenario starts from.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    /// Test-owned scene setups, admitted through the same boundary as the game.
    #[serde(default)]
    pub(crate) scenes: Vec<arpg_game::GameScene>,
    /// Ordered placements/removals after authored scenes load. Interleaving them
    /// allows stale-identity tests to reuse a retired slot.
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
    /// Optional scene load index; absent selects setup.sources.
    #[serde(default)]
    pub(crate) scene: Option<usize>,
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

    /// Grants seeking to a body identified by placement number.
    Seek(usize),
}

/// A queued spawn at a specified tick using the sim-owned template.
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

/// Holds a world-space direction for a span of ticks, independent of the camera.
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

/// Changes the driver's retained aim before the named tick.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AimAt {
    pub(crate) at: u64,
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
    /// Mean time in arpg_game::Game::step only; excludes setup, hashing and assertions.
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
    /// Player health remaining; zero does not yet end the scenario.
    #[serde(default)]
    pub(crate) player_health: Option<u8>,
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
    /// Nonphysical remains currently retained by Game.
    #[serde(default)]
    pub(crate) corpse_count: Option<usize>,
    /// How many bodies chase the player. The cheapest way to assert that two
    /// bodies made from different templates were granted different behaviours.
    #[serde(default)]
    pub(crate) seekers: Option<usize>,
    /// How many sources are still live at the end.
    #[serde(default)]
    pub(crate) sources: Option<usize>,
    #[serde(default)]
    pub(crate) source_states: Vec<SourceExpect>,
    #[serde(default)]
    pub(crate) encounters: Vec<EncounterExpect>,
    #[serde(default)]
    pub(crate) doors: Vec<DoorExpect>,
    #[serde(default)]
    pub(crate) chests: Vec<ChestExpect>,
    #[serde(default)]
    pub(crate) scene_count: Option<usize>,
    #[serde(default)]
    pub(crate) tree_count: Option<usize>,
    #[serde(default)]
    pub(crate) torch_count: Option<usize>,
    #[serde(default)]
    pub(crate) torches: Vec<TorchExpect>,
    /// Predictions about individual bodies placed by [`Setup::actions`].
    #[serde(default)]
    pub(crate) bodies: Vec<BodyExpect>,
    /// Exact trace fixture, relative to the scenario. Prefer typed `events` for
    /// mechanics; reserve fixtures for serialization/order. Review `--bless` diffs.
    #[serde(default)]
    pub(crate) trace: Option<String>,
    /// Exact sequence among explicitly selected event kinds; other diagnostics do not matter.
    #[serde(default)]
    pub(crate) events: Option<crate::events::Events>,
}

/// A predicted scalar and how far off it may be.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approx {
    #[serde(deserialize_with = "finite")]
    pub(crate) value: f32,
    /// **For float accumulation, never for timing slop.** Under a fixed
    /// timestep the tick count is exact, so a tolerance wide enough to absorb a
    /// one-tick error is not a tolerance — it is the assertion being switched
    /// off. One tick of walking is 0.15 world units.
    #[serde(deserialize_with = "tolerance")]
    pub(crate) tol: f32,
}

/// A predicted ground-plane position and how far off it may be.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approx2 {
    #[serde(deserialize_with = "finite")]
    pub(crate) x: f32,
    #[serde(deserialize_with = "finite")]
    pub(crate) z: f32,
    /// Euclidean distance, so this is a radius rather than a per-axis slack.
    #[serde(deserialize_with = "tolerance")]
    pub(crate) tol: f32,
}

/// Prediction fields reject invalid numbers at deserialization, including in
/// checkpoints and event payloads. NaN must never disable a comparison.
pub(crate) fn finite<'de, D: serde::Deserializer<'de>>(input: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(input)?;
    if !value.is_finite() {
        return Err(serde::de::Error::custom("prediction must be finite"));
    }
    Ok(value)
}

pub(crate) fn tolerance<'de, D: serde::Deserializer<'de>>(input: D) -> Result<f32, D::Error> {
    let value = finite(input)?;
    if value < 0.0 {
        return Err(serde::de::Error::custom("tolerance must be nonnegative"));
    }
    Ok(value)
}

pub(crate) fn finite_pair<'de, D: serde::Deserializer<'de>>(input: D) -> Result<(f32, f32), D::Error> {
    let value = <(f32, f32)>::deserialize(input)?;
    if !value.0.is_finite() || !value.1.is_finite() {
        return Err(serde::de::Error::custom("prediction must be finite"));
    }
    Ok(value)
}

impl Approx {
    pub(crate) fn matches(&self, got: f32) -> bool {
        got.is_finite() && (got - self.value).abs() <= self.tol
    }
}

impl Approx2 {
    /// How far `got` misses by, or `None` if it is within tolerance.
    ///
    /// Non-finite observations fail too. Double precision keeps subtraction
    /// and distance finite across the full range of authored finite f32 values.
    pub(crate) fn off_by(&self, got: glam::Vec3) -> Option<f32> {
        let off = (f64::from(got.x) - f64::from(self.x))
            .hypot(f64::from(got.z) - f64::from(self.z));
        (!off.is_finite() || off > f64::from(self.tol)).then_some(off as f32)
    }

    /// The prediction, as it appears in a failure.
    pub(crate) fn expected(&self) -> String {
        format!("({:.4}, {:.4}) +/- {:.4}", self.x, self.z, self.tol)
    }
}

/// What must hold for one body placed by an [`Action::Place`] in [`Setup::actions`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BodyExpect {
    /// Stable placement number, resolved through the recorded entity ID rather than a dense row.
    pub(crate) nth: usize,

    /// Where the body should be. Omit to say nothing about position.
    #[serde(default)]
    pub(crate) pos: Option<Approx2>,
    #[serde(default)]
    pub(crate) velocity: Option<Approx2>,

    /// Remaining hits before defeat. Only live enemy bodies have health.
    #[serde(default)]
    pub(crate) health: Option<u8>,

    /// Whether the body has health membership at all.
    #[serde(default)]
    pub(crate) damageable: Option<bool>,

    /// Sim-owned one-shot interaction state.
    #[serde(default)]
    pub(crate) interaction: Option<arpg_sim::InteractionState>,

    /// Whether the body should be chasing the player.
    ///
    /// `seeking: false` on a body that was never granted it is the control in
    /// the pair: a pass that moved everything would satisfy every assertion
    /// about the chaser and only fail here.
    #[serde(default)]
    pub(crate) seeking: Option<bool>,

    /// Simulation-owned locomotion choice for the latest tick.
    #[serde(default)]
    pub(crate) gait: Option<arpg_sim::Gait>,

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SceneAt {
    pub(crate) at: u64,
    pub(crate) action: SceneAction,
}

/// Indices refer to load order, including setup, never to a content name.
#[derive(Debug, Deserialize)]
pub(crate) enum SceneAction {
    Load(Box<arpg_game::GameScene>),
    Evict(usize),
}

/// A removal uses the same placement identity binding as body assertions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DespawnAt {
    pub(crate) at: u64,
    pub(crate) body: usize,
}

/// Exact game-owned torch observation, addressed by scene installation and authored index.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TorchExpect {
    pub(crate) scene: usize,
    pub(crate) torch: usize,
    pub(crate) state: Option<arpg_game::TorchSpec>,
}

/// Exact authoritative encounter observation, indexed in its owning scene.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EncounterExpect {
    pub(crate) scene: usize,
    pub(crate) encounter: usize,
    pub(crate) state: Option<arpg_game::EncounterState>,
}
/// Exact authoritative door observation, including removal and eviction.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DoorExpect {
    pub(crate) scene: usize,
    pub(crate) door: usize,
    pub(crate) state: Option<arpg_game::DoorState>,
}

/// Exact authoritative chest observation, including disappearance and eviction.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChestExpect {
    pub(crate) scene: usize,
    pub(crate) chest: usize,
    pub(crate) state: Option<arpg_game::ChestState>,
}


#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TuningAt {
    pub(crate) at: u64,
    pub(crate) values: arpg_sim::Tuning,
}
