use crate::tree::{Trees, TreeSpec};
use crate::scene::{GameScene, LiveSceneState, RestartError, RestartScene, SceneError, fingerprint};
use arpg_core::{Intent, Report};
use crate::torch::{Torches, TorchSpec};
use crate::chest::{self, Chests};
use crate::door::{self, Doors};
use crate::encounter::{self, Encounters, EncounterEvent};
use crate::enemy_attack::{self, EnemyAttacks};
use arpg_sim::{
    Alpha, AttackProfile, AttackStatus, Dt, EntityId, Fnv, Impulse, InteractionState, Motion,
    RecoveryTicks, SceneId, Source, SourceId, Template, Trace, World,
};
use glam::{Vec2, Vec3};

/// Complete playable state and the entry point used by production and tests.
///
/// Engine state is private so callers cannot step it around the game schedule.
/// Complete run replacement and the restart snapshot are owned here. Live
/// instances delegate engine cleanup through exhaustive lifecycle boundaries;
/// adding gameplay storage requires an explicit decision at those boundaries.
///
/// ```compile_fail
/// let mut game = arpg_game::Game::empty();
/// let _ = &mut game.world;
/// ```
pub struct Game {
    world: World,
    restart: Option<RestartScene>,
    trees: Trees,
    torches: Torches,
    doors: Doors,
    chests: Chests,
    encounters: Encounters,
    encounter_trace: Trace<EncounterEvent>,
    attacks: EnemyAttacks,
    corpses: crate::corpse::Corpses,
}

impl Default for Game {
    /// Constructs the built-in horde through ordinary scene loading.
    fn default() -> Self {
        let mut game = Self::empty();
        game.load_scene(&GameScene::boot()).expect("valid boot scene");
        game
    }
}

impl Game {
    /// A fresh player without content, at tick zero with default tuning.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            world: World::empty(),
            restart: None,
            trees: Trees::default(),
            torches: Torches::default(),
            doors: Doors::default(),
            chests: Chests::default(),
            encounters: Encounters::default(),
            encounter_trace: Trace::default(),
            attacks: EnemyAttacks::default(),
            corpses: crate::corpse::Corpses::default(),
        }
    }

    /// Constructs a fresh game from a validated scene without running ticks.
    pub fn from_scene(scene: &GameScene) -> Result<Self, SceneError> {
        let crate::scene::PreparedScene { world, trees, torches, doors, chests, encounters, attacks, restart } = RestartScene::prepare(scene)?;
        Ok(Self {
            world,
            restart: Some(restart),
            trees,
            torches,
            doors,
            chests,
            encounters,
            encounter_trace: Trace::default(),
            attacks,
            corpses: crate::corpse::Corpses::default(),
        })
    }

    /// Starts a fresh run and selects its restart snapshot, atomically.
    ///
    /// Failure leaves all current state, pending work, identities, trace, and
    /// the previous snapshot untouched. Success replaces the whole Game, so
    /// future gameplay fields cannot survive by being omitted from a reset list.
    /// Runtime identities are scoped to the new run, as with `from_scene`.
    pub fn start_scene(&mut self, scene: &GameScene) -> Result<(), SceneError> {
        let replacement = Self::from_scene(scene)?;
        *self = replacement;
        Ok(())
    }

    /// Restarts from the cached description without consulting the filesystem.
    /// Additive loads and eviction never change the selected snapshot.
    pub fn restart(&mut self) -> Result<(), RestartError> {
        let snapshot = self.restart.as_ref().ok_or(RestartError::NoScene)?;
        let replacement = Self::from_scene(snapshot.description())
            .map_err(RestartError::InvalidScene)?;
        *self = replacement;
        Ok(())
    }

    /// Label of the selected restart snapshot, even if its live instance is evicted.
    #[must_use]
    pub fn selected_scene_name(&self) -> Option<&str> {
        self.restart.as_ref().map(|snapshot| snapshot.description().engine.name.as_str())
    }

    /// Consumes prior interactions, steps physics, then resolves encounters.
    ///
    /// Neither adapter owns a second schedule. Exhaustive destructuring forces
    /// any future gameplay state to receive an explicit scheduling decision.
    pub fn step(&mut self, dt: Dt, intent: Intent) {
        self.step_observed(dt, intent, &mut ());
    }

    /// The ordinary game schedule with read-only engine timing boundaries.
    pub fn step_observed(&mut self, dt: Dt, intent: Intent, observer: &mut impl arpg_sim::StepObserver) {
        let Self {
            world,
            restart: _,
            trees: _,
            torches: _,
            doors,
            chests,
            encounters,
            encounter_trace,
            attacks,
            corpses,
        } = self;
        let tick = world.tick();
        let (mut access, _) = world.encounter_access();
        let mut trace = encounter_trace.sink(tick);
        door::advance(doors, &mut access, &mut trace);
        chest::advance(chests, &mut access, &mut trace);
        encounter::begin(encounters, doors, chests, &mut access, &mut trace);
        world.step_observed(dt, intent, observer);
        crate::corpse::advance(corpses, world.defeats());
        enemy_attack::advance(attacks, world.melee_view(), trace.reborrow());
        let (mut access, defeats) = world.encounter_access();
        encounter::finish(encounters, doors, chests, defeats, &mut access, &mut trace);
    }

    /// Hashes engine state, live relationships, and the cached restart effect.
    #[must_use]
    pub fn hash(&self) -> u64 {
        let Self {
            world,
            restart,
            trees,
            torches,
            doors,
            chests,
            encounters,
            encounter_trace: _,
            attacks,
            corpses,
        } = self;
        let mut hash = Fnv::default();
        hash.u64(fingerprint(world, (torches, trees), doors, chests, encounters, attacks));
        corpses.hash(&mut hash);
        hash.usize(usize::from(restart.is_some()));
        if let Some(snapshot) = restart {
            snapshot.hash(&mut hash);
        }
        hash.finish()
    }

    /// Engine-only fingerprint for comparing physical state independently of restart.
    #[must_use]
    pub fn engine_hash(&self) -> u64 {
        self.world.hash()
    }

    /// Reports engine state, resolved gameplay relationships, and restart selection.
    pub fn report(&self, out: &mut Report) {
        let Self {
            world,
            restart,
            trees,
            torches,
            doors,
            chests,
            encounters,
            encounter_trace,
            attacks,
            corpses,
        } = self;
        world.report(out);
        trees.report(out);
        torches.report(out);
        doors.report(out);
        chests.report(out);
        encounters.report(out);
        attacks.report(out);
        corpses.report(out);
        out.int("encounter_events_dropped", encounter_trace.dropped() as u64);
        out.object("restart", |out| {
            out.bool("available", restart.is_some());
            if let Some(snapshot) = restart {
                snapshot.report(out);
            }
        });
    }

    /// Installs a scene between ticks through the engine admission boundary.
    pub fn load_scene(&mut self, scene: &GameScene) -> Result<SceneId, SceneError> {
        let Self {
            world,
            restart: _,
            trees,
            torches,
            doors,
            chests,
            encounters,
            encounter_trace: _,
            attacks,
            corpses: _, // Additive loads retain existing corpses.
        } = self;
        scene.install(world, LiveSceneState { trees, torches, doors, chests, encounters, attacks })
    }

    /// Evicts a live instance and its descendants; preserves the restart snapshot.
    #[must_use]
    pub fn evict_scene(&mut self, id: SceneId) -> bool {
        // Both owners retire at this boundary; callers cannot omit one cleanup.
        let Self {
            world,
            restart: _,
            trees,
            torches,
            doors,
            chests,
            encounters,
            encounter_trace: _,
            attacks,
            corpses,
        } = self;
        if !world.evict_scene(id) {
            return false;
        }
        trees.evict(id);
        torches.evict(id);
        doors.evict(id);
        chests.evict(id);
        encounters.evict(id);
        attacks.evict(id);
        corpses.evict(id);
        true
    }

    /// Solid trees in scene-installation and authored order.
    pub fn trees(&self) -> impl Iterator<Item = TreeSpec> + '_ { self.trees.iter() }

    /// Stationary torches in stable scene-installation and authored order.
    pub fn torches(&self) -> impl Iterator<Item = TorchSpec> + '_ { self.torches.iter() }

    /// Number of lights across all live scenes.
    pub fn torch_count(&self) -> usize { self.torches.len() }

    /// A scene-local torch, or None for an evicted scene or missing index.
    pub fn scene_torch(&self, scene: SceneId, nth: usize) -> Option<TorchSpec> {
        self.torches.state(scene, nth)
    }

    /// Nonphysical remains in admission order.
    pub fn corpses(&self) -> &[crate::Corpse] { self.corpses.as_slice() }

    /// Number of live scene instances.
    #[must_use]
    pub fn scene_count(&self) -> usize {
        self.world.scene_count()
    }

    /// Live scene identities and labels in creation order.
    pub fn scene_instances(&self) -> impl Iterator<Item = (SceneId, &str)> {
        self.world.scene_instances()
    }

    /// Bodies owned by a live scene, including its emitted descendants.
    #[must_use]
    pub fn scene_bodies(&self, id: SceneId) -> Option<&[EntityId]> {
        self.world.scene_bodies(id)
    }

    /// Places a validated body between ticks; None means placement was refused.
    #[must_use]
    pub fn place(&mut self, at: Vec2, what: Template) -> Option<EntityId> {
        self.world.place(at, what)
    }

    /// Removes a body through the engine lifetime boundary; false means refused.
    pub fn despawn_body(&mut self, id: EntityId) -> bool {
        if !self.world.despawn_body(id) { return false; }
        self.attacks.remove_body(id);
        true
    }

    /// Grants seeking only if the engine accepts the body capability.
    pub fn add_seek(&mut self, id: EntityId) -> bool {
        self.world.add_seek(id)
    }

    /// Requests a spawn at the next structural boundary; `None` means queue refusal.
    #[must_use]
    pub fn request_spawn(&mut self, at: Vec2, what: Template) -> Option<arpg_sim::SpawnTicket> {
        self.world.request_spawn(at, what)
    }

    /// Authoritative queued spawn results from the latest completed tick.
    #[must_use]
    pub fn spawn_outcomes(&self) -> &[arpg_sim::SpawnOutcome] { self.world.spawn_outcomes() }

    /// Installs a source between ticks and returns its stable identity.
    #[must_use]
    pub fn add_source(&mut self, source: Source) -> SourceId {
        self.world.add_source(source)
    }

    /// Removes a source through the engine lifetime boundary; false means absent.
    pub fn remove_source(&mut self, id: SourceId) -> bool {
        self.world.remove_source(id)
    }

    /// Applies a validated impulse; false means the target cannot receive it.
    #[must_use]
    pub fn apply_impulse(&mut self, id: EntityId, impulse: Impulse) -> bool {
        self.world.apply_impulse(id, impulse)
    }

    /// Selects the profile for the next swing without changing a committed swing.
    pub fn set_attack_profile(&mut self, profile: AttackProfile) {
        self.world.set_attack_profile(profile);
    }

    /// Sets validated recovery for the next swing through the shared tuning door.
    pub fn set_attack_recovery(&mut self, recovery: RecoveryTicks) {
        self.world.set_attack_recovery(recovery);
    }

    /// Configure engine diagnostic history; zero disables recent history.
    /// Unsupported limits leave every stream unchanged.
    pub fn set_trace_retention(&mut self, capacity: usize) -> Result<(), &'static str> {
        Trace::<arpg_sim::Event>::validate_limit(capacity)?;
        self.world.set_trace_retention(capacity)?;
        self.encounter_trace.set_retention(capacity)
    }

    /// Collect all diagnostic streams independently of their retention rings.
    /// Unsupported limits preserve all existing capture and history.
    pub fn capture_trace(&mut self, limit: usize) -> Result<(), &'static str> {
        Trace::<arpg_sim::Event>::validate_limit(limit)?;
        self.world.capture_trace(limit)?;
        self.encounter_trace.capture(limit, |_| true)
    }

    /// Render complete captured streams, or fail closed on any overflow.
    pub fn render_captured_trace(&self) -> Option<String> {
        Some(render_events(
            self.world.trace().captured()?.iter().copied(),
            self.encounter_trace.captured()?.iter().copied(),
        ))
    }

    /// Collect selected engine transitions separately from recent history.
    pub fn capture_events(&mut self, limit: usize, kinds: Vec<arpg_sim::EventKind>) -> Result<(), &'static str> {
        self.world.capture_events(limit, kinds)?;
        // A filtered engine stream cannot be rendered as a complete multi-stream log.
        self.encounter_trace.stop_capture();
        Ok(())
    }

    /// Complete selected evidence, or `None` if its bound was exceeded.
    pub fn captured_events(&self) -> Option<&[(u64, arpg_sim::Event)]> {
        self.world.trace().captured()
    }

    /// Clears diagnostic history without changing simulation state or its hash.
    pub fn clear_trace(&mut self) {
        self.world.clear_trace();
        self.encounter_trace.clear();
    }

    /// Reads tick-stamped engine events; observation cannot mutate them.
    #[must_use]
    pub fn trace(&self) -> &Trace {
        self.world.trace()
    }

    /// Number of completed fixed ticks.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.world.tick()
    }

    /// Number of enemies, excluding static props.
    #[must_use]
    pub fn enemy_count(&self) -> usize {
        self.world.enemy_count()
    }

    /// Number of bodies with seeking membership.
    #[must_use]
    pub fn seeker_count(&self) -> usize {
        self.world.seeker_count()
    }

    /// Number of live sources.
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.world.source_count()
    }

    /// Reads a source's live cadence/enablement state without exposing storage.
    #[must_use]
    pub fn source_state(&self, id: SourceId) -> Option<arpg_sim::SourceState> {
        self.world.source_state(id)
    }

    /// Enables or pauses a source through the engine's validated identity door.
    #[must_use]
    pub fn set_source_enabled(&mut self, id: SourceId, enabled: bool) -> bool {
        self.world.set_source_enabled(id, enabled)
    }

    /// Current attack configuration and committed swing state.
    #[must_use]
    pub fn attack_status(&self) -> AttackStatus {
        self.world.attack_status()
    }

    /// Whether the current swing has an active hitbox.
    #[must_use]
    pub fn hitbox_is_live(&self) -> bool {
        self.world.hitbox_is_live()
    }

    /// Number of bodies struck by the current or most recent swing.
    #[must_use]
    pub fn struck(&self) -> usize {
        self.world.struck()
    }

    /// Player simulation position in world metres.
    #[must_use]
    pub fn player_pos(&self) -> Vec3 {
        self.world.player_pos()
    }

    /// Immutable player facts for asset-driven presentation.
    #[must_use]
    pub fn player_presentation(&self, alpha: Alpha) -> arpg_sim::PlayerPresentation {
        self.world.player_presentation(alpha)
    }

    /// Immutable, allocation-free presentation facts for live enemies.
    pub fn enemy_presentations(
        &self,
        alpha: Alpha,
    ) -> impl Iterator<Item = arpg_sim::EnemyPresentation> + '_ {
        self.world.enemy_presentations(alpha)
    }

    /// Authored ground-plane wall geometry and scene-local identities.
    pub fn walls(&self) -> impl Iterator<Item = (SceneId, usize, arpg_sim::Wall)> + '_ { self.world.walls() }

    /// Wall meshes only; tree-owned rectangles are presented by their tree mesh.
    /// Physical queries and collision observations still include all rectangles.
    pub fn visible_walls(&self) -> impl Iterator<Item = (SceneId, usize, arpg_sim::Wall)> + '_ {
        self.world.walls().filter(|(owner, index, _)| !self.trees.owns_wall(*owner, *index))
    }

    /// Active beam clipped to the first wall at the completed tick.
    pub fn beam(&self) -> Option<arpg_sim::Beam> { self.world.beam() }

    /// Read-only nearest wall contact, using the shared physical geometry.
    pub fn wall_hit(
        &self,
        start: Vec2,
        end: Vec2,
    ) -> Result<Option<arpg_sim::WallHit>, &'static str> {
        self.world.wall_hit(start, end)
    }

    /// Completed-tick beam damage positions, including targets defeated that tick.
    pub fn beam_hits(&self) -> &[Vec2] { self.world.beam_hits() }

    /// Committed swing discs; beam geometry has its own read-only observation.
    pub fn attack_discs(&self) -> impl Iterator<Item = arpg_sim::AttackDisc> + '_ {
        self.world.attack_discs()
    }

    /// Authoritative collision geometry, without presentation interpolation.
    pub fn collision_discs(&self) -> impl Iterator<Item = arpg_sim::CollisionDisc> + '_ {
        self.world.collision_discs()
    }

    /// Immutable prop facts for asset-driven presentation.
    pub fn prop_presentations(&self, alpha: Alpha) -> impl Iterator<Item = arpg_sim::PropPresentation> + '_ {
        self.world.prop_presentations(alpha)
    }

    /// Player simulation facing in radians.
    #[must_use]
    pub fn player_facing(&self) -> f32 {
        self.world.player_facing()
    }

    /// Stable identity of the player body.
    #[must_use]
    pub fn player_id(&self) -> EntityId {
        self.world.player_id()
    }

    /// Resolves a harness body name without forging an entity identity.
    #[must_use]
    pub fn body_named(&self, name: &str) -> Option<EntityId> {
        self.world.body_named(name)
    }

    /// World-space position of a live body.
    #[must_use]
    pub fn body_pos(&self, id: EntityId) -> Option<Vec3> {
        self.world.body_pos(id)
    }

    /// Whether the complete generational identity still names a live body.
    #[must_use]
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.world.is_alive(id)
    }

    /// Health for a body with that capability; None includes static props.
    #[must_use]
    pub fn health(&self, id: EntityId) -> Option<u8> {
        self.world.health(id)
    }

    /// Player health remaining. Reaching zero does not yet end or interrupt play.
    #[must_use]
    pub fn player_health(&self) -> u8 {
        self.attacks.player_health()
    }

    /// Read-only physical payload for a body with motion membership.
    #[must_use]
    pub fn motion(&self, id: EntityId) -> Option<Motion> {
        self.world.motion(id)
    }

    /// Ground gait currently selected by a live chaser.
    #[must_use]
    pub fn gait(&self, id: EntityId) -> Option<arpg_sim::Gait> {
        self.world.gait(id)
    }

    /// Whether a live identity has seeking membership.
    #[must_use]
    pub fn is_seeker(&self, id: EntityId) -> bool {
        self.world.is_seeker(id)
    }

    /// Interaction state for a body with that capability.
    #[must_use]
    pub fn interaction_state(&self, id: EntityId) -> Option<InteractionState> {
        self.world.interaction_state(id)
    }

    /// Player contacts resolved in the last tick.
    #[must_use]
    pub fn contacts(&self) -> usize {
        self.world.contacts()
    }

    /// Crowd contacts resolved in the last tick.
    #[must_use]
    pub fn crowd_contacts(&self) -> usize {
        self.world.crowd_contacts()
    }

    /// Checks the engine position invariant for the headless gate.
    #[must_use]
    pub fn all_positions_finite(&self) -> bool {
        self.world.all_positions_finite()
    }
    /// Sources in current installation order; removals prune this list.
    #[must_use]
    pub fn scene_sources(&self, scene: SceneId) -> Option<&[SourceId]> {
        self.world.scene_sources(scene)
    }

    /// Dropped events across all bounded diagnostic streams.
    #[must_use]
    pub fn trace_dropped(&self) -> usize {
        self.world.trace().dropped() + self.encounter_trace.dropped()
    }

    /// Engine diagnostics followed by a labelled gameplay stream. Each stream
    /// preserves its own event order; equal ticks across streams do not invent
    /// a chronology between external commands and the gameplay pass.
    #[must_use]
    pub fn render_trace_since(&self, since: u64) -> String {
        render_events(self.world.trace().since(since), self.encounter_trace.since(since))
    }
}

impl Game {
    /// Observe one encounter by scene identity and authored index.
    pub fn encounter_state(&self, scene: SceneId, nth: usize) -> Option<crate::EncounterState> { self.encounters.state(scene, nth) }
    /// Scene-installation order, then authored order; presentation cannot mutate these facts.
    pub fn encounters(&self) -> impl Iterator<Item = (&str, crate::EncounterState)> { self.encounters.iter() }
    /// Observe a scene-owned door, including terminal Open or Missing state.
    pub fn door_state(&self, scene: SceneId, nth: usize) -> Option<crate::DoorState> { self.doors.state(scene, nth) }
    /// Lossless combat outcomes of the latest completed engine tick.
    pub fn defeats(&self) -> &[arpg_sim::Defeat] { self.world.defeats() }
    /// Encounter, door and chest diagnostics, separate from authoritative progression.
    pub fn encounter_trace(&self) -> &Trace<EncounterEvent> { &self.encounter_trace }
}

impl Game {
    /// Scene-local authoritative chest state, absent after eviction.
    pub fn chest_state(&self, scene: SceneId, nth: usize) -> Option<crate::ChestState> {
        self.chests.state(scene, nth)
    }
    /// Chest role for a stable physical prop identity; presentation cannot change it.
    pub fn chest_state_for_body(&self, body: EntityId) -> Option<crate::ChestState> {
        self.chests.for_body(body)
    }
}

/// Streams have an explicit order, rather than inventing cross-stream chronology.
fn render_events(
    engine: impl Iterator<Item = (u64, arpg_sim::Event)>,
    encounters: impl Iterator<Item = (u64, EncounterEvent)>,
) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (tick, event) in engine {
            let _ = writeln!(out, "{tick} {event}");
        }
        let mut encounters = encounters.peekable();
        if encounters.peek().is_some() {
            out.push_str("# encounters\n");
            for (tick, event) in encounters { let _ = writeln!(out, "{tick} {event}"); }
        }
        out
}
