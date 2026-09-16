use crate::scene::{GameScene, RestartError, RestartScene, SceneError, fingerprint};
use crate::source_control::{self, ControlEvent, ControlState, SourceControls};
use arpg_core::{Intent, Report};
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
    controls: SourceControls,
    control_trace: Trace<ControlEvent>,
}

impl Default for Game {
    /// Preserves the engine's default horde, including its identity history.
    fn default() -> Self {
        Self {
            world: World::default(),
            restart: None,
            controls: SourceControls::default(),
            control_trace: Trace::default(),
        }
    }
}

impl Game {
    /// A fresh player without content, at tick zero with default tuning.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            world: World::empty(),
            restart: None,
            controls: SourceControls::default(),
            control_trace: Trace::default(),
        }
    }

    /// Constructs a fresh game from a validated scene without running ticks.
    pub fn from_scene(scene: &GameScene) -> Result<Self, SceneError> {
        let (world, controls, restart) = RestartScene::prepare(scene)?;
        Ok(Self {
            world,
            restart: Some(restart),
            controls,
            control_trace: Trace::default(),
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

    /// Advances source control, then one fixed tick of the engine schedule.
    ///
    /// Neither adapter owns a second schedule. Exhaustive destructuring forces
    /// any future gameplay state to receive an explicit scheduling decision.
    pub fn step(&mut self, dt: Dt, intent: Intent) {
        let Self {
            world,
            restart: _,
            controls,
            control_trace,
        } = self;
        let tick = world.tick();
        let (interactions, sources) = world.interaction_sources();
        source_control::advance(controls, interactions, sources, control_trace.sink(tick));
        world.step(dt, intent);
    }

    /// Hashes engine state, live relationships, and the cached restart effect.
    #[must_use]
    pub fn hash(&self) -> u64 {
        let Self {
            world,
            restart,
            controls,
            control_trace: _,
        } = self;
        let mut hash = Fnv::default();
        hash.u64(fingerprint(world, controls));
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
            controls,
            control_trace,
        } = self;
        world.report(out);
        controls.report(out);
        out.int("control_events_dropped", control_trace.dropped() as u64);
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
            controls,
            control_trace: _,
        } = self;
        scene.install(world, controls)
    }

    /// Evicts a live instance and its descendants; preserves the restart snapshot.
    #[must_use]
    pub fn evict_scene(&mut self, id: SceneId) -> bool {
        // Both owners retire at this boundary; callers cannot omit one cleanup.
        let Self {
            world,
            restart: _,
            controls,
            control_trace: _,
        } = self;
        if !world.evict_scene(id) {
            return false;
        }
        controls.evict(id);
        true
    }

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

    /// Applies the existing debug population reset, preserving props and sources.
    pub fn set_enemy_count(&mut self, n: usize) {
        self.world.set_enemy_count(n);
    }

    /// Sets debug seeker membership through the engine capability boundary.
    pub fn set_seeker_count(&mut self, n: usize) {
        self.world.set_seeker_count(n);
    }

    /// Places a validated body between ticks; None means placement was refused.
    #[must_use]
    pub fn place(&mut self, at: Vec2, what: Template) -> Option<EntityId> {
        self.world.place(at, what)
    }

    /// Removes a body through the engine lifetime boundary; false means refused.
    pub fn despawn_body(&mut self, id: EntityId) -> bool {
        self.world.despawn_body(id)
    }

    /// Grants seeking only if the engine accepts the body capability.
    pub fn add_seek(&mut self, id: EntityId) -> bool {
        self.world.add_seek(id)
    }

    /// Requests a spawn at the next structural boundary; false means queue refusal.
    #[must_use]
    pub fn request_spawn(&mut self, at: Vec2, what: Template) -> bool {
        self.world.request_spawn(at, what)
    }

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

    /// Clears diagnostic history without changing simulation state or its hash.
    pub fn clear_trace(&mut self) {
        self.world.clear_trace();
        self.control_trace.clear();
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

    /// Player presentation position interpolated between completed ticks.
    #[must_use]
    pub fn player_pos_at(&self, alpha: Alpha) -> Vec3 {
        self.world.player_pos_at(alpha)
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

    /// Read-only committed attack geometry at the completed tick's player pose.
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

    /// Read-only physical payload for a body with motion membership.
    #[must_use]
    pub fn motion(&self, id: EntityId) -> Option<Motion> {
        self.world.motion(id)
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
    /// Reads a scene-local relationship and its source's authoritative state.
    #[must_use]
    pub fn source_control_state(&self, scene: SceneId, nth: usize) -> Option<ControlState> {
        self.controls.state(scene, nth, &self.world)
    }

    /// Sources in current installation order; removals prune this list.
    #[must_use]
    pub fn scene_sources(&self, scene: SceneId) -> Option<&[SourceId]> {
        self.world.scene_sources(scene)
    }

    /// Gameplay diagnostics are separate typed events, never pass input.
    #[must_use]
    pub fn control_trace(&self) -> &Trace<ControlEvent> { &self.control_trace }

    /// Dropped events across both bounded diagnostic streams.
    #[must_use]
    pub fn trace_dropped(&self) -> usize {
        self.world.trace().dropped() + self.control_trace.dropped()
    }

    /// Engine diagnostics followed by a labelled gameplay stream. Each stream
    /// preserves its own event order; equal ticks across streams do not invent
    /// a chronology between external commands and the gameplay pass.
    #[must_use]
    pub fn render_trace_since(&self, since: u64) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (tick, event) in self.world.trace().since(since) {
            let _ = writeln!(out, "{tick} {event}");
        }
        let mut gameplay = self.control_trace.since(since).peekable();
        if gameplay.peek().is_some() {
            out.push_str("# gameplay transitions\n");
            for (tick, event) in gameplay { let _ = writeln!(out, "{tick} {event}"); }
        }
        out
    }
}
