//! Complete authored gameplay and its immutable restart snapshot.

use arpg_core::Report;
use crate::set_piece::{SetPieces, SetPieceSpec};
use arpg_sim::{Fnv, SceneId, World};
use serde::Deserialize;
use crate::chest::{ChestSpec, Chests};
use crate::door::{DoorSpec, Doors};
use crate::encounter::{EncounterSpec, Encounters};
use crate::enemy_attack::{EnemyAttackSpec, EnemyAttacks};
use crate::torch::{Torches, TorchSpec};

/// Authored gameplay wraps the engine definition without copying its fields.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameScene {
    /// Physical content, still defined and validated by sim.
    pub engine: arpg_sim::Scene,
    /// Stationary lights owned by this scene instance. Omitted means no lights.
    #[serde(default)]
    pub torches: Vec<TorchSpec>,
    /// Optional finite moments, activated explicitly by scene identity and name.
    #[serde(default)]
    pub set_pieces: Vec<SetPieceSpec>,
    /// Locked doors owned by this scene instance.
    #[serde(default)]
    pub doors: Vec<DoorSpec>,
    /// Locked chests that remain present after opening.
    #[serde(default)]
    pub chests: Vec<ChestSpec>,
    /// Independently instantiated gameplay encounters.
    #[serde(default)]
    pub encounters: Vec<EncounterSpec>,
    /// The one melee attacker supported by the initial damage slice.
    #[serde(default)]
    pub enemy_attack: Option<EnemyAttackSpec>,
}

impl From<arpg_sim::Scene> for GameScene {
    fn from(engine: arpg_sim::Scene) -> Self {
        Self { engine, torches: Vec::new(), set_pieces: Vec::new(), doors: Vec::new(), chests: Vec::new(), encounters: Vec::new(), enemy_attack: None }
    }
}

impl GameScene {
    /// The built-in engine horde without gameplay connections.
    #[must_use]
    pub fn boot() -> Self {
        arpg_sim::Scene::boot().into()
    }

    /// The only installation path, shared by fresh runs and additive loads.
    /// Validate relationships before physical admission can mutate the world.
    pub(crate) fn install(&self, world: &mut World, live: LiveSceneState<'_>) -> Result<SceneId, SceneError> {
        let LiveSceneState { torches, set_pieces, doors, chests, encounters, attacks } = live;
        Doors::validate(&self.doors, &self.engine.bodies)?;
        chests.validate(&self.chests, &self.engine.bodies, &self.doors)?;
        encounters.validate_add(self)?;
        torches.validate_add(&self.torches)?;
        set_pieces.validate_add(&self.set_pieces)?;
        attacks.validate_add(self.enemy_attack, &self.engine.bodies)?;
        let owner = world.load_scene(&self.engine)?;
        doors.install(owner, &self.doors, world.scene_bodies(owner).expect("admitted scene"));
        chests.install(owner, &self.chests, world.scene_bodies(owner).expect("admitted scene"));
        encounters.install(owner, &self.encounters, world.scene_bodies(owner).expect("admitted scene"), world.scene_sources(owner).expect("admitted scene"));
        attacks.install(owner, self.enemy_attack, world.scene_bodies(owner).expect("admitted scene"));
        torches.install(owner, &self.torches);
        set_pieces.install(owner, &self.set_pieces);
        Ok(owner)
    }
}

/// The complete set of gameplay stores that participate in scene admission.
/// Keeping this private prevents ordinary passes from using it as mutable Game.
pub(crate) struct LiveSceneState<'a> {
    pub(crate) torches: &'a mut Torches,
    pub(crate) set_pieces: &'a mut SetPieces,
    pub(crate) doors: &'a mut Doors,
    pub(crate) chests: &'a mut Chests,
    pub(crate) encounters: &'a mut Encounters,
    pub(crate) attacks: &'a mut EnemyAttacks,
}

/// Rejection of a complete scene, before any partial installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneError {
    /// Engine content or capacity admission failed.
    Engine(arpg_sim::SceneError),
    /// The complete scene would exceed the shared light capacity.
    Lighting(&'static str),
    /// Invalid set-piece names or excess reserved effects.
    SetPiece(&'static str),
    /// Invalid encounter or door composition.
    Encounter(&'static str),
    /// Invalid or excess enemy-attack relationship.
    EnemyAttack(&'static str),
}

impl From<arpg_sim::SceneError> for SceneError {
    fn from(error: arpg_sim::SceneError) -> Self { Self::Engine(error) }
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(error) => error.fmt(out),
            Self::Lighting(reason) | Self::SetPiece(reason) | Self::Encounter(reason)
            | Self::EnemyAttack(reason) => out.write_str(reason),
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

/// Complete validated replacement, ready for atomic installation by Game.
pub(crate) struct PreparedScene {
    pub(crate) world: World,
    pub(crate) torches: Torches,
    pub(crate) set_pieces: SetPieces,
    pub(crate) doors: Doors,
    pub(crate) chests: Chests,
    pub(crate) encounters: Encounters,
    pub(crate) attacks: EnemyAttacks,
    pub(crate) restart: RestartScene,
}

impl RestartScene {
    /// Prepare a complete replacement before either the run or snapshot changes.
    pub(crate) fn prepare(scene: &GameScene) -> Result<PreparedScene, SceneError> {
        let mut world = World::empty();
        let mut torches = Torches::default();
        let mut set_pieces = SetPieces::default();
        let mut doors = Doors::default();
        let mut chests = Chests::default();
        let mut encounters = Encounters::default();
        let mut attacks = EnemyAttacks::default();
        scene.install(&mut world, LiveSceneState {
            torches: &mut torches, set_pieces: &mut set_pieces, doors: &mut doors,
            chests: &mut chests, encounters: &mut encounters, attacks: &mut attacks,
        })?;
        let initial_hash = fingerprint(&world, &torches, &set_pieces, &doors, &chests, &encounters, &attacks);
        let snapshot = Self { scene: scene.clone(), initial_hash };
        Ok(PreparedScene { world, torches, set_pieces, doors, chests, encounters, attacks, restart: snapshot })
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

/// Hash every gameplay store in a fixed order, including empty stores.
/// Each store encodes its length or presence, so adjacent stores stay distinct.
pub(crate) fn fingerprint(world: &World, torches: &Torches, set_pieces: &SetPieces, doors: &Doors, chests: &Chests, encounters: &Encounters, attacks: &EnemyAttacks) -> u64 {
    let mut hash = Fnv::default();
    torches.hash(&mut hash);
    set_pieces.hash(&mut hash);
    doors.hash(&mut hash);
    chests.hash(&mut hash);
    encounters.hash(&mut hash);
    attacks.hash(&mut hash);
    hash.u64(world.hash());
    hash.finish()
}
