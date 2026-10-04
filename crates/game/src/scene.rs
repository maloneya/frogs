//! Complete authored gameplay and its immutable restart snapshot.

use crate::environment::{EnvironmentSpec, Environments};
use crate::tree::{Trees, TreeSpec};

use arpg_core::Report;
use arpg_sim::{Fnv, SceneId, World};
use serde::{Deserialize, Serialize};
use crate::chest::{ChestSpec, Chests};
use crate::door::{DoorSpec, Doors};
use crate::encounter::{EncounterSpec, Encounters};
use crate::behavior::Actors;
use crate::population::Births;
use crate::torch::{Torches, TorchSpec};

/// Complete authored actor content, decoration and gameplay relationships.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameScene {
    /// Scene-wide illumination, independent of the number of torches.
    #[serde(default)]
    pub environment: EnvironmentSpec,
    /// Actor births and physical geometry; sim validates their physical projection.
    pub engine: crate::Scene,
    /// Stationary lights owned by this scene instance. Omitted means no lights.
    #[serde(default)]
    pub torches: Vec<TorchSpec>,
    /// Trees owned by this scene, including their solid trunk colliders.
    #[serde(default)]
    pub trees: Vec<TreeSpec>,
    /// Gameplay bindings to initially latched physical panels.
    #[serde(default)]
    pub doors: Vec<DoorSpec>,
    /// Locked chests that remain present after opening.
    #[serde(default)]
    pub chests: Vec<ChestSpec>,
    /// Independently instantiated gameplay encounters.
    #[serde(default)]
    pub encounters: Vec<EncounterSpec>,
}

impl From<crate::Scene> for GameScene {
    fn from(engine: crate::Scene) -> Self {
        Self { engine, ..Self::default() }
    }
}

impl GameScene {
    /// The only installation path, shared by fresh runs and additive loads.
    /// Validate relationships before physical admission can mutate the world.
    pub(crate) fn install(&self, world: &mut World, live: LiveSceneState<'_>) -> Result<SceneId, SceneError> {
        let LiveSceneState { trees, torches, environments, doors, chests, encounters, actors, births } = live;
        Doors::validate(&self.doors, &self.engine.bodies)?;
        chests.validate(&self.chests, &self.engine.bodies, &self.doors)?;
        encounters.validate_add(self)?;
        trees.validate_add(&self.trees)?;
        torches.validate_add(&self.torches)?;

        // Append trunk shapes after authored walls so authored wall identities
        // and body indices stay stable. Engine admission validates the combined
        // wall capacity before mutating any live state.
        let mut physical = self.engine.physical().map_err(SceneError::Behavior)?;
        physical.walls.extend(self.trees.iter().map(|tree| tree.collider()));
        physical.panels.extend(self.doors.iter().map(|door| arpg_sim::PanelSpec {
            body: door.body, config: door.panel,
        }));
        let owner = world.load_scene(&physical)?;
        doors.install(owner, &self.doors, world.scene_bodies(owner).expect("admitted scene"));
        chests.install(owner, &self.chests, world.scene_bodies(owner).expect("admitted scene"));
        encounters.install(owner, &self.encounters, world.scene_bodies(owner).expect("admitted scene"), world.scene_sources(owner).expect("admitted scene"));
        births.install(&self.engine, owner, world.scene_bodies(owner).expect("admitted scene"), world.scene_sources(owner).expect("admitted scene"), actors);
        trees.install(owner, self.engine.walls.len(), &self.trees);
        torches.install(owner, &self.torches);
        environments.install(owner, self.environment);
        Ok(owner)
    }
}

/// The complete set of gameplay stores that participate in scene admission.
/// Keeping this private prevents ordinary passes from using it as mutable Game.
pub(crate) struct LiveSceneState<'a> {
    pub(crate) environments: &'a mut Environments,
    pub(crate) trees: &'a mut Trees,
    pub(crate) torches: &'a mut Torches,
    pub(crate) doors: &'a mut Doors,
    pub(crate) chests: &'a mut Chests,
    pub(crate) encounters: &'a mut Encounters,
    pub(crate) actors: &'a mut Actors,
    pub(crate) births: &'a mut Births,
}

/// Rejection of a complete scene, before any partial installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneError {
    /// Engine content or capacity admission failed.
    Engine(arpg_sim::SceneError),
    /// The complete scene would exceed the shared light capacity.
    Lighting(&'static str),
    /// Scene decoration capacity exceeded.
    Decoration(&'static str),
    /// Invalid encounter or door composition.
    Encounter(&'static str),
    /// Invalid actor definition or behavior composition.
    Behavior(&'static str),
}

impl From<arpg_sim::SceneError> for SceneError {
    fn from(error: arpg_sim::SceneError) -> Self { Self::Engine(error) }
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(error) => error.fmt(out),
            Self::Decoration(reason) | Self::Lighting(reason) | Self::Encounter(reason)
            | Self::Behavior(reason) => out.write_str(reason),
        }
    }
}
impl std::error::Error for SceneError {}


/// Immutable, validated authored content and the fingerprint of its fresh state.
/// The fingerprint is computed at the only construction door, never per tick.
pub(crate) struct RestartScene {
    scene: GameScene,
    content_hash: u64,
}

/// Complete validated replacement, ready for atomic installation by Game.
pub(crate) struct PreparedScene {
    pub(crate) environments: Environments,
    pub(crate) world: World,
    pub(crate) trees: Trees,
    pub(crate) torches: Torches,
    pub(crate) doors: Doors,
    pub(crate) chests: Chests,
    pub(crate) encounters: Encounters,
    pub(crate) actors: Actors,
    pub(crate) births: Births,
    pub(crate) restart: RestartScene,
}

impl RestartScene {
    /// Prepare a complete replacement before either the run or snapshot changes.
    pub(crate) fn prepare(scene: &GameScene) -> Result<PreparedScene, SceneError> {
        let mut world = World::empty();
        let mut trees = Trees::default();
        let mut torches = Torches::default();
        let mut environments = Environments::default();
        let mut doors = Doors::default();
        let mut chests = Chests::default();
        let mut encounters = Encounters::default();
        let mut actors = Actors::default();
        let mut births = Births::default();
        scene.install(&mut world, LiveSceneState {
            trees: &mut trees, torches: &mut torches, environments: &mut environments, doors: &mut doors,
            chests: &mut chests, encounters: &mut encounters, actors: &mut actors, births: &mut births,
        })?;
        let content_hash = fingerprint(&world, (&torches, &trees, &environments), &doors, &chests, &encounters, &actors, &births);
        let snapshot = Self { scene: scene.clone(), content_hash };
        Ok(PreparedScene { world, trees, torches, environments, doors, chests, encounters, actors, births, restart: snapshot })
    }

    pub(crate) fn description(&self) -> &GameScene {
        &self.scene
    }

    pub(crate) fn hash(&self, out: &mut Fnv) {
        let Self { scene: _, content_hash } = self;
        // This fingerprints content instantiated with built-in tuning. Live tuning
        // is hashed by World and survives restart separately. Equivalent
        // authored spellings can share a fingerprint because their future effect
        // is identical. The snapshot cannot be edited after this is computed.
        out.u64(*content_hash);
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { scene, content_hash } = self;
        out.text("name", &scene.engine.name);
        out.text("content_hash", &format!("{content_hash:016x}"));
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

/// Hash every gameplay store in a fixed order, including empty stores.
/// Each store encodes its length or presence, so adjacent stores stay distinct.
pub(crate) fn fingerprint(world: &World, decorations: (&Torches, &Trees, &Environments), doors: &Doors, chests: &Chests, encounters: &Encounters, actors: &Actors, births: &Births) -> u64 {
    let mut hash = Fnv::default();
    decorations.0.hash(&mut hash);
    decorations.1.hash(&mut hash);
    decorations.2.hash(&mut hash);
    doors.hash(&mut hash);
    chests.hash(&mut hash);
    encounters.hash(&mut hash);
    actors.hash(&mut hash);
    births.hash(&mut hash);
    hash.u64(world.hash());
    hash.finish()
}

impl GameScene {
    /// Remove an explicit body without retargeting gameplay links. Referenced
    /// bodies must remain until their relationship is removed by its author.
    pub fn remove_body(&mut self, index: usize) -> Result<(), &'static str> {
        use crate::EncounterStart;
        if index >= self.engine.bodies.len() { return Err("body no longer exists"); }
        if self.doors.iter().any(|s| s.body == index)
            || self.chests.iter().any(|s| s.body == index)
            || self.encounters.iter().any(|s| matches!(s.start, EncounterStart::Activated(i) if i == index))
        { return Err("This body is used by a door, chest or encounter and cannot be deleted."); }
        self.engine.bodies.remove(index);
        for s in &mut self.doors { if s.body > index { s.body -= 1; } }
        for s in &mut self.chests { if s.body > index { s.body -= 1; } }
        for s in &mut self.encounters {
            let EncounterStart::Activated(i) = &mut s.start;
            if *i > index { *i -= 1; }
        }
        Ok(())
    }

    /// Remove an unreferenced source and remap surviving encounter groups.
    pub fn remove_source(&mut self, index: usize) -> Result<(), &'static str> {
        use crate::EncounterObjective;
        if index >= self.engine.sources.len() { return Err("source no longer exists"); }
        if self.encounters.iter().any(|s| {
            let EncounterObjective::Defeat(goal) = &s.objective;
            goal.sources.contains(&index)
        }) { return Err("This source belongs to an encounter and cannot be deleted."); }
        self.engine.sources.remove(index);
        for s in &mut self.encounters {
            let EncounterObjective::Defeat(goal) = &mut s.objective;
            for i in &mut goal.sources { if *i > index { *i -= 1; } }
        }
        Ok(())
    }
}
