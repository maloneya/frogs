//! Complete authored gameplay and its immutable restart snapshot.

use arpg_core::Report;
use arpg_sim::{Fnv, SceneId, World};
use serde::Deserialize;
use crate::torch::{Torches, TorchSpec};
use crate::source_control::{SourceControlSpec, SourceControls};

/// Authored gameplay wraps the engine definition without copying its fields.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameScene {
    /// Physical content, still defined and validated by sim.
    pub engine: arpg_sim::Scene,
    /// Scene-local one-shot connections, validated before any engine mutation.
    #[serde(default)]
    pub source_controls: Vec<SourceControlSpec>,
    /// Stationary lights owned by this scene instance. Omitted means no lights.
    #[serde(default)]
    pub torches: Vec<TorchSpec>,
}

impl From<arpg_sim::Scene> for GameScene {
    fn from(engine: arpg_sim::Scene) -> Self {
        Self { engine, source_controls: Vec::new(), torches: Vec::new() }
    }
}

impl GameScene {
    /// The built-in engine horde without gameplay connections.
    #[must_use]
    pub fn boot() -> Self {
        Self { torches: vec![TorchSpec::boot()], ..arpg_sim::Scene::boot().into() }
    }

    /// The only installation path, shared by fresh runs and additive loads.
    /// Validate relationships before physical admission can mutate the world.
    pub(crate) fn install(&self, world: &mut World, controls: &mut SourceControls, torches: &mut Torches) -> Result<SceneId, SceneError> {
        self.validate_controls()?;
        torches.validate_add(&self.torches)?;
        let owner = world.load_scene(&self.engine)?;
        controls.install(owner, &self.source_controls,
            world.scene_bodies(owner).expect("admitted scene owns its bodies"),
            world.scene_sources(owner).expect("admitted scene owns its sources"));
        torches.install(owner, &self.torches);
        Ok(owner)
    }

    fn validate_controls(&self) -> Result<(), SceneError> {
        let Self { engine, source_controls, torches: _ } = self;
        for (nth, spec) in source_controls.iter().enumerate() {
            let SourceControlSpec { body, source } = *spec;
            if !engine.bodies.get(body).is_some_and(|body| body.what.interactable()) {
                return Err(SceneError::Control("control body must name an explicit interactable body"));
            }
            if !engine.sources.get(source).is_some_and(|source| !source.enabled) {
                return Err(SceneError::Control("control source must name an initially disabled source"));
            }
            if source_controls[..nth].iter().any(|other| other.source == source) {
                return Err(SceneError::Control("a source may have only one activation controller"));
            }
        }
        Ok(())
    }
}

/// Rejection of a complete scene, before any partial installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneError {
    /// Engine content or capacity admission failed.
    Engine(arpg_sim::SceneError),
    /// A gameplay relationship was invalid.
    Control(&'static str),
    /// The complete scene would exceed the shared light capacity.
    Lighting(&'static str),
}

impl From<arpg_sim::SceneError> for SceneError {
    fn from(error: arpg_sim::SceneError) -> Self { Self::Engine(error) }
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(error) => error.fmt(out),
            Self::Control(reason) | Self::Lighting(reason) => out.write_str(reason),
        }
    }
}
impl std::error::Error for SceneError {}


/// Immutable, validated authored content and the fingerprint of its fresh state.
/// The fingerprint is computed at the only construction door, never per tick.
pub(crate) struct RestartScene {
    scene: GameScene,
    initial_hash: u64,
}

impl RestartScene {
    /// Prepare a complete replacement before either the run or snapshot changes.
    pub(crate) fn prepare(scene: &GameScene) -> Result<(World, SourceControls, Torches, Self), SceneError> {
        let mut world = World::empty();
        let mut controls = SourceControls::default();
        let mut torches = Torches::default();
        scene.install(&mut world, &mut controls, &mut torches)?;
        let initial_hash = fingerprint(&world, &controls, &torches);
        let snapshot = Self { scene: scene.clone(), initial_hash };
        Ok((world, controls, torches, snapshot))
    }

    pub(crate) fn description(&self) -> &GameScene {
        &self.scene
    }

    pub(crate) fn hash(&self, out: &mut Fnv) {
        let Self { scene: _, initial_hash } = self;
        // Restart reconstructs precisely this validated initial world. Equivalent
        // authored spellings can share a fingerprint because their future effect
        // is identical. The snapshot cannot be edited after this is computed.
        out.u64(*initial_hash);
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { scene, initial_hash } = self;
        out.text("name", &scene.engine.name);
        out.text("initial_hash", &format!("{initial_hash:016x}"));
    }
}

/// A restart needs a selected snapshot and a valid complete replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartError {
    /// Empty/default games have no selected scene until one is started.
    NoScene,
    /// Rebuilding the snapshot failed; the active game is unchanged.
    InvalidScene(SceneError),
}

impl std::fmt::Display for RestartError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoScene => out.write_str("No current playtest"),
            Self::InvalidScene(error) => error.fmt(out),
        }
    }
}

impl std::error::Error for RestartError {}

/// No relationship means exactly the established physical fingerprint.
pub(crate) fn fingerprint(world: &World, controls: &SourceControls, torches: &Torches) -> u64 {
    if controls.is_empty() && torches.len() == 0 { return world.hash(); }
    let mut hash = Fnv::default();
    controls.hash(&mut hash);
    if torches.len() > 0 { torches.hash(&mut hash); }
    hash.u64(world.hash());
    hash.finish()
}
