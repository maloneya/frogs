//! What exists, and how it describes itself to a renderer.
//!
//! Depends on `arpg-core` for vocabulary and on nothing else. In particular it
//! does not link wgpu, so simulation tests run without a GPU.
//!
//! Spatial values follow the world-unit convention in [`arpg_core`]: one world
//! unit is one metre, including scene inputs and reported positions. Linear
//! speeds are metres per second. Tick counts remain counts, not seconds.

// New simulation states must not disappear into a catch-all match arm.
#![deny(clippy::wildcard_enum_match_arm)]

use glam::{Vec2, Vec3};

use arpg_core::Report;

mod action_execution;
pub mod angle;
mod attack;
pub use action_execution::{
    ActionControls, ActionExecution, ActionOutcome, ActionSchedule, ActionTimeline, Interruption,
};
mod beam;
mod contact_memory;
mod joint;
mod solver;
mod wall;
pub use beam::Beam;
pub use joint::{
    HingeConfig, Impact, JointConfig, JointPresentation, JointSpec, JointState, LockState,
    LockStrength, MAX_JOINTS, ToppleConfig,
};
pub use wall::{MAX_WALLS, Wall, WallHit};
mod grid;
mod hash;
mod members;
mod observe;
pub use observe::{CrowdWork, PhysicsCounter, PhysicsPhase, StepObserver};
mod pass;
mod presentation;
mod scene;
mod slots;
mod swing;
pub use swing::{Disc, Hitbox};
mod time;
mod trace;

pub use attack::{
    AttackPhase, AttackResolveError, AttackShape, RecoveryTicks,
    ResolvedAttack,
};
pub use hash::Fnv;
use members::Members;
pub use pass::health::Defeat;
use pass::health::Health;
use pass::interact::Interactions;
pub use pass::interact::{INTERACTION_REACH, InteractionState};
use pass::motion::Physics;
pub use pass::motion::{Impulse, Mass, Motion};
pub use pass::motion::{MotorCommand, motor_speed_limit, stopping_speed};
/// Read-only snapshot of a source's enablement, cadence, and emission progress.
pub use pass::source::SourceState;
use pass::source::Sources;
/// Who asks for spawns, and when. See [`crate::pass::source`] for why a source
/// is its own thing rather than a behaviour on a body.
pub use pass::source::{Condition, Source, SourceId, SourceSpec};
use pass::spawn::SpawnQueue;
/// What to make, and what behaviours it should be granted. See
/// [`crate::pass::spawn`] for why spawning is a queue rather than a call.
pub use pass::spawn::{SpawnOutcome, SpawnTicket, Template};
pub use presentation::{
    AttackDisc, CharacterPresentation, CollisionDisc, PropPresentation,
};
use scene::Scenes;
pub use scene::{BodyGrid, Placed, Scene, SceneError, SceneId};
pub use slots::EntityId;
use slots::Slots;
pub use time::{Accumulator, Alpha, Dt, TICK_HZ, Ticks};
pub use trace::{Event, EventKind, MAX_TRACE_EVENTS, Trace, TraceSink};
mod access;
pub use access::{ActorView, DefeatedBodyRetirement, GameplayAccess};
mod step;
pub use step::{PreparedStep, ResolvedStep};

/// Initial health of the persistent player body.
pub const PLAYER_MAX_HEALTH: u8 = 5;

mod tuning;
pub use tuning::ContactMaterial;

/// Converts ground X/Z to world XYZ at the supplied height.
fn on_ground(p: Vec2, height: f32) -> Vec3 {
    Vec3::new(p.x, height, p.y)
}

/// Maximum non-player physical population, shared by enemies and props.
///
/// Preserves the former admission ceiling of 183,494 bodies. This is now an
/// explicit simulation bound, independent of renderer buffers and hitbox length.
/// Presentation checks its GPU capacity against this bound at compile time.
pub const MAX_BODIES: usize = 183_494;
const _: () = assert!(MAX_BODIES > 0);

/// Legacy report height retained independently of the rendered asset dimensions.
const ENEMY_HALF_HEIGHT: f32 = 0.25;

/// Disc collision radii in metres, independent of visible mesh dimensions.
pub const ENEMY_RADIUS: f32 = 0.25;

/// Player collision footprint, independent of the authored mesh.
pub const PLAYER_RADIUS: f32 = 0.3;

const _: () = assert!(ENEMY_RADIUS > 0.0 && PLAYER_RADIUS > 0.0);
/// Collision radius of a body row; row 0 is always the player.
pub(crate) fn body_radius(row: usize) -> f32 {
    if row == 0 {
        PLAYER_RADIUS
    } else {
        ENEMY_RADIUS
    }
}
/// Half the arena width, in metres. Presentation scales its ground to this extent.
pub const ARENA_HALF: f32 = 96.0;
const _: () = assert!(ARENA_HALF.is_finite() && ARENA_HALF > PLAYER_RADIUS);

/// Legacy player report height, preserved independently of asset dimensions.
const PLAYER_HALF_HEIGHT: f32 = 0.6;

/// Grow a scratch buffer to at least `capacity` without shrinking it.
pub(crate) fn reserve_capacity<T>(buffer: &mut Vec<T>, capacity: usize) {
    if buffer.capacity() < capacity {
        buffer.reserve(capacity - buffer.len());
    }
}

const MAX_POSITION_COMPONENT: f32 = 4.0e18;

/// Numeric position admission shared by body placement and gameplay destinations.
/// The conservative component bound keeps the squared separation of any two
/// admitted positions below f32::MAX (8 * (4e18)^2), before arena containment.
/// This is an arithmetic domain, not a restriction to the playable arena.
pub fn valid_position(position: Vec2) -> bool {
    position.is_finite() && position.abs().max_element() <= MAX_POSITION_COMPONENT
}

/// Shared body storage. Row zero is the persistent player.
/// Separate position arrays keep unrelated state out of solver iteration.
struct Bodies {
    physics: Physics,
    interactions: Interactions,
    /// Health is independent of movement, presentation and gameplay allegiance.
    health: Health,
    characters: Members,
    /// Stable identities mapped to dense rows.
    slots: Slots,
    /// Ground positions: X is world X, Y is world Z; [`on_ground`] converts to 3D.
    pos: Vec<Vec2>,
    /// Previous-tick positions for interpolation, aligned with `pos`.
    prev_pos: Vec<Vec2>,
}

impl Default for Bodies {
    fn default() -> Self {
        let mut bodies = Self {
            physics: Physics::default(),
            interactions: Interactions::default(),
            health: Health::default(),
            characters: Members::default(),
            slots: Slots::default(),
            pos: Vec::new(),
            prev_pos: Vec::new(),
        };
        let mass = Mass::UNIT;
        bodies
            .spawn(Vec2::ZERO, Template::BODY.with_mass(mass))
            .expect("the initial player fits");
        bodies
    }
}

impl Bodies {
    fn len(&self) -> usize {
        self.pos.len()
    }

    /// Spawns a body with both interpolation endpoints at `at`, preventing a
    /// one-frame streak when rendering occurs before the next simulation tick.
    fn spawn(&mut self, at: Vec2, what: Template) -> Option<EntityId> {
        if self.len() > MAX_BODIES || !what.valid_at(at) {
            return None;
        }
        let Self {
            physics,
            health,
            characters,
            interactions,
            slots,
            pos,
            prev_pos,
        } = self;
        let is_player = pos.is_empty();
        let id = slots.insert();
        physics.grant(
            id,
            if what.fixed() {
                0.0
            } else {
                what.mass().kilograms().recip()
            },
        );
        pos.push(at);
        prev_pos.push(at);
        if !is_player && what.interactable() {
            interactions.grant(id);
        }
        if is_player {
            health.grant(
                id,
                core::num::NonZeroU8::new(PLAYER_MAX_HEALTH).expect("positive player health"),
            );
        } else if let Some(initial) = what.hit_points() {
            health.grant(id, initial);
        }
        if !is_player && what.is_character() {
            characters.add(id);
        }
        self.debug_check_paired();
        Some(id)
    }

    /// Debug-only alignment checks for storage owned here.
    fn debug_check_paired(&self) {
        debug_assert_eq!(
            self.slots.len(),
            self.physics.len(),
            "physics and bodies disagree"
        );
        debug_assert_eq!(self.slots.len(), self.pos.len(), "slots and pos disagree");
        debug_assert_eq!(
            self.slots.len(),
            self.prev_pos.len(),
            "slots and prev_pos disagree"
        );
        // Capability rows belong to their own membership sets. Checking their
        // count is constant time; rescanning all ids on each spawn was quadratic.
        debug_assert!(
            self.health.len() <= self.slots.len(),
            "health belongs to live bodies"
        );
    }

    /// Removes one body. Returns whether the id named a live one.
    ///
    /// Dense arrays swap-remove the row returned by `Slots`; sparse capabilities
    /// revoke the identity. Exhaustive destructuring makes adding a body field
    /// require a cleanup decision here for every removal.
    fn despawn(&mut self, id: EntityId) -> bool {
        let Self {
            physics,
            health,
            characters,
            interactions,
            slots,
            pos,
            prev_pos,
        } = self;
        if slots.index(id) == Some(0) {
            return false;
        }
        let Some(dense) = slots.remove(id) else {
            return false;
        };

        physics.revoke(id);
        interactions.revoke(id);
        health.revoke(id);
        characters.remove(id);
        pos.swap_remove(dense);
        prev_pos.swap_remove(dense);
        self.debug_check_paired();
        true
    }

    /// Where a named body stands, or `None` if it is dead.
    fn pos_of(&self, id: EntityId) -> Option<Vec2> {
        self.slots.index(id).map(|i| self.pos[i])
    }
}

/// What exists: the horde, the player, and the bookkeeping the two seams —
/// [`World::player_presentation`] and [`World::trace`] — hand out.
pub struct World {
    material: ContactMaterial,
    bodies: Bodies,
    /// Simulation ticks completed since this world was created.
    ///
    /// The index everything an agent needs to observe is keyed by: trace
    /// events are stamped with it, scenario inputs are scheduled at it, and a
    /// replay divergence is reported as one. It counts *completed* steps, so
    /// during a step it names the tick being run, 0-based.
    ///
    /// `u64` because wrapping is not a failure mode anyone should have to
    /// think about: at 60Hz this overflows in roughly ten billion years.
    tick: u64,
    /// What the simulation *did*, as opposed to what it now holds.
    ///
    /// Lives on `World` rather than beside it because passes write to it, and a
    /// pass takes the things it declares — a trace threaded in from `app` would
    /// be unavailable to the scenario runner, which is the consumer that
    /// matters most.
    trace: Trace,
    /// Player-against-horde contacts resolved by the last physical tick.
    ///
    /// The instrument the collision work is measured with. A body is nine
    /// pixels across at this zoom and a contact displaces it by a fraction of
    /// that, so "is anything actually touching" is invisible on screen and
    /// obvious as a number. It also checks broad-phase compatibility:
    /// a grid that resolves different contacts than brute force is wrong,
    /// and this is what catches it.
    contacts: usize,
    /// What has been asked for and not yet made.
    ///
    /// **The seam that lets anything ask for a spawn without being able to
    /// perform one.** Spawning moves rows, and every pass in the schedule holds
    /// an index into them, so exactly one pass grants — first, before anything
    /// reads a position. See [`crate::pass::spawn`], which carries the argument.
    queue: SpawnQueue,
    /// Everything that asks for spawns, and the state each needs to decide.
    ///
    /// A list of its own rather than a behaviour attached to a body: a source
    /// is what *makes* bodies, so hanging it off one of its products inverts
    /// the layering and breaks as soon as the thing being made is not a body.
    /// See [`crate::pass::source`].
    sources: Sources,
    scenes: Scenes,
    /// Lossless outcomes of the latest tick, replaced only by the next step.
    defeats: Vec<Defeat>,
    /// Non-player pairs overlapping in the last physical tick.
    ///
    /// Kept apart from `contacts` rather than summed into it: one number
    /// covering both cannot distinguish the player wading into a pack from the
    /// pack settling on its own, and it is the crowd half specifically that the
    /// uniform grid accelerates.
    crowd_contacts: usize,
    /// Rebuilt per pass; scratch contains no persistent identities or simulation facts.
    crowd_grid: grid::Grid,
}

impl Default for World {
    fn default() -> Self {
        Self::empty()
    }
}

impl World {
    /// A fresh player and no content, at tick zero with default contact response.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            material: ContactMaterial::default(),
            bodies: Bodies::default(),
            tick: 0,
            trace: Trace::default(),
            queue: SpawnQueue::default(),
            sources: Sources::default(),
            scenes: Scenes::default(),
            defeats: Vec::with_capacity(MAX_BODIES),
            contacts: 0,
            crowd_contacts: 0,
            crowd_grid: grid::Grid::default(),
        }
    }
}

/// The one implementation of non-player body removal, shared by between-tick callers and
/// the final scheduled defeat pass.
fn remove_body(
    bodies: &mut Bodies,
    scenes: &mut Scenes,
    id: EntityId,
    trace: &mut trace::TraceSink<'_>,
) -> Option<()> {
    if !bodies.despawn(id) {
        return None;
    }
    scenes.forget_body(id);
    trace.emit(Event::Removed { id });
    Some(())
}

impl World {
    /// Nonplayer bodies with health, independent of gameplay allegiance. Always within
    /// `0..=MAX_BODIES`.
    ///
    /// Derived from the storage rather than tracked beside it: a separate
    /// counter is a second copy of the same fact, and the two drift the first
    /// time something spawns or kills one without going through the same door.
    pub fn damageable_count(&self) -> usize {
        self.bodies.health.ids().iter().filter(|&&id| {
            id != self.player_id() && self.bodies.health.get(id) != Some(0)
        }).count()
    }

    /// Non-player bodies, including props; all share the instance capacity.
    #[must_use]
    pub fn body_count(&self) -> usize {
        self.bodies.len() - 1
    }

    /// Immediately places a body at world X/Z coordinates between ticks.
    /// Returns `None` for invalid input or exhausted [`MAX_BODIES`] capacity.
    /// Use [`World::request_spawn`] for queued placement.
    pub fn place(&mut self, at: Vec2, what: Template) -> Option<EntityId> {
        let mut trace = self.trace.sink(self.tick);
        pass::spawn::place(&mut self.bodies, at, what, &mut trace)
    }

    /// Adds something that asks for spawns, and returns its name.
    ///
    /// Sources are evaluated at the top of every tick, in the order they were
    /// added — which is the order they take slots in, so a replay puts bodies
    /// in the same places.
    pub fn add_source(&mut self, source: Source) -> SourceId {
        let id = self.sources.add(source);

        // Traced because it happens outside the schedule, and because a source
        // is the explanation for every body it goes on to make.
        self.trace.sink(self.tick).emit(Event::SourceAdded { id });
        id
    }

    /// Removes a source. Returns whether it named a live one.
    ///
    /// **This is what "destroying a spawner stops the flow" means**, and it is
    /// deliberately not tied to killing a body: whatever a game decides ends a
    /// source — a body dying, a room clearing, a timer — calls this.
    pub fn remove_source(&mut self, id: SourceId) -> bool {
        if !self.sources.remove(id) {
            return false;
        }
        self.scenes.forget_source(id);
        self.trace.sink(self.tick).emit(Event::SourceRemoved { id });
        true
    }

    /// How many sources are live.
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Reads a live source's state; a retired or unknown identity resolves to None.
    #[must_use]
    pub fn source_state(&self, id: SourceId) -> Option<SourceState> {
        self.sources.state(id)
    }

    /// Sets enablement before the next source evaluation. Disabled cadence is
    /// frozen; re-enabling never resets it. Returns false only for an absent id.
    /// Repeated settings succeed without adding trace events or changing state.
    #[must_use]
    pub fn set_source_enabled(&mut self, id: SourceId, enabled: bool) -> bool {
        self.sources
            .set_enabled(id, enabled, self.trace.sink(self.tick))
    }

    /// Queues a body for the next tick and returns its stable ticket.
    /// A full queue returns `None` and emits a refusal on the next tick.
    #[must_use]
    pub fn request_spawn(&mut self, at: Vec2, what: Template) -> Option<SpawnTicket> {
        self.queue.push(at, what)
    }

    /// Results of all accepted queued requests drained in the latest tick, in FIFO order.
    /// Replaced by the next step; independent of diagnostic trace retention.
    #[must_use]
    pub fn spawn_outcomes(&self) -> &[SpawnOutcome] {
        self.queue.outcomes()
    }

    /// Removes one non-player body and its capabilities. Returns whether it existed.
    ///
    /// A stale id is a no-op rather than an error: something holding a
    /// reference to a body that has already died is the normal case, not a
    /// mistake, and it is exactly what [`EntityId`]'s generation makes safe to
    /// ask about.
    pub fn despawn_body(&mut self, id: EntityId) -> bool {
        // Exhaustive for the same reason as scene eviction: the other door a
        // new behaviour has to be considered at. A line per behaviour rather
        // than a registry is deliberate — a list of sets to sweep would be a
        // second hand-maintained record of which behaviours exist.
        // `queue` is deliberately untouched: a request names a place and a
        // template, never a body, so nothing pending can refer to the name
        // being retired here.
        let Self {
            bodies,
            trace,
            tick,
            queue: _,
            sources: _,
            scenes,
            defeats,
            contacts: _,
            crowd_contacts: _,
            crowd_grid: _,
            material: _,
        } = self;

        let _ = defeats; // Between-tick removals never manufacture combat outcomes.
        let mut trace = trace.sink(*tick);
        remove_body(bodies, scenes, id, &mut trace).is_some()
    }

    /// Remaining health, or `None` for immune bodies and stale identities.
    #[must_use]
    pub fn health(&self, id: EntityId) -> Option<u8> {
        self.bodies.health.get(id)
    }

    /// Initial health capacity of a live body, independent of its current health.
    pub fn max_health(&self, id: EntityId) -> Option<u8> {
        self.bodies.health.maximum(id)
    }

    /// The state of a live interaction member, or None for an ordinary body.
    #[must_use]
    pub fn interaction_state(&self, id: EntityId) -> Option<InteractionState> {
        self.bodies.interactions.get(id)
    }

    /// Replace validated global contact response between ticks.
    pub fn set_contact_material(&mut self, material: ContactMaterial) { self.material = material; }
    /// Change an existing body's mass through the shared validated physical type.
    pub fn set_mass(&mut self, body: EntityId, mass: Mass) -> bool {
        if !self.actor_view().movable(body) { return false; }
        self.bodies.physics.set_mass(body, mass);
        true
    }

    /// Where a named body stands, lifted to world space, or `None` if it is
    /// dead or names the player. The height is a constant of the body's size rather than state —
    /// see [`Bodies::pos`].
    #[must_use]
    pub fn body_pos(&self, id: EntityId) -> Option<Vec3> {
        if id == self.player_id() {
            return None;
        }
        self.bodies
            .pos_of(id)
            .map(|p| on_ground(p, ENEMY_HALF_HEIGHT))
    }

    /// Whether this id still names a live body.
    #[must_use]
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.bodies.slots.contains(id)
    }

    /// Describes itself for an agent, field by field.
    ///
    /// **The same exhaustive destructuring as [`World::hash`], for the same
    /// reason.** A field added to `World` fails to compile until it is
    /// reported, so "anything an agent must observe is a derived field" is a
    /// rule the compiler applies rather than one someone remembers.
    pub fn report(&self, out: &mut Report) {
        let Self {
            bodies,
            tick,
            queue,
            sources,
            scenes,
            defeats,
            contacts,
            crowd_contacts,
            crowd_grid: _,
            material,
            trace,
        } = self;

        out.object("contact_material", |out| {
            out.num("frequency_hz", material.frequency_hz());
            out.num("damping_ratio", material.damping_ratio());
            out.num("push_speed", material.push_speed());
        });
        bodies.physics.report(&bodies.pos, &bodies.slots, out);
        bodies.health.report(out);
        bodies.interactions.report(out);
        out.object("defeats", |out| {
            for defeat in defeats {
                out.object(&defeat.body.to_string(), |out| {
                    out.num("x", defeat.position.x);
                    out.num("z", defeat.position.y);
                    if let Some(scene) = defeat.scene {
                        out.text("scene", &scene.to_string());
                    }
                    if let Some(source) = defeat.source {
                        out.text("source", &source.to_string());
                    }
                });
            }
        });
        out.int("tick", *tick);
        out.vec3("player_pos", on_ground(bodies.pos[0], PLAYER_HALF_HEIGHT));
        out.int("contacts", *contacts as u64);
        out.int("crowd_contacts", *crowd_contacts as u64);

        // Derived, and reported because "everything is still a number" is not
        // visible in any of the values above — a NaN position prints as a
        // position and compares equal to nothing.
        out.bool("finite", self.all_positions_finite());
        out.int("damageable", self.damageable_count() as u64);
        out.int("props", (bodies.len() - 1 - bodies.characters.len()) as u64);
        out.int("characters", bodies.characters.len() as u64);
        // Asked for and not yet made. Without it, "the spawn has not happened
        // yet" and "the spawn was refused" look identical from out here — and
        // one of those is a bug.
        out.int("queued", queue.len() as u64);
        out.object("spawn_outcomes", |out| {
            for result in queue.outcomes() {
                let SpawnOutcome {
                    ticket,
                    entity,
                    owner,
                    source,
                } = result;
                out.object(&ticket.to_string(), |out| {
                    out.bool("spawned", entity.is_some());
                    if let Some(id) = entity {
                        out.text("entity", &id.to_string());
                    }
                    if let Some(id) = owner {
                        out.text("owner", &id.to_string());
                    }
                    if let Some(id) = source {
                        out.text("source", &id.to_string());
                    }
                });
            }
        });
        out.int("sources", sources.len() as u64);
        out.object("source_states", |out| sources.report(out));
        out.int("scene_count", scenes.len() as u64);
        out.object("scenes", |out| scenes.report(out));

        out.int("trace_events", trace.len() as u64);
        out.int("trace_dropped", trace.dropped() as u64);

        // Previous-tick state is what render interpolation blends from. Not
        // interesting most of the time, and exactly the thing to look at when
        // something on screen is a tick behind where it should be.
        out.vec3(
            "player_prev_pos",
            on_ground(bodies.prev_pos[0], PLAYER_HALF_HEIGHT),
        );
    }

    /// Discards the trace so far. See [`Trace::clear`] for when that is right.
    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    /// Configure recent diagnostic history without changing simulation state.
    pub fn set_trace_retention(&mut self, capacity: usize) -> Result<(), &'static str> {
        self.trace.set_retention(capacity)
    }

    /// Capture all engine events for diagnostic serialization checks.
    pub fn capture_trace(&mut self, limit: usize) -> Result<(), &'static str> {
        self.trace.capture_all(limit)
    }

    /// Capture selected engine transitions independently of diagnostic history.
    pub fn capture_events(
        &mut self,
        limit: usize,
        kinds: Vec<EventKind>,
    ) -> Result<(), &'static str> {
        self.trace
            .capture_selected(limit, move |event| kinds.contains(&event.kind()))
    }

    /// What the simulation has done recently, oldest first.
    /// Read-only: perception cannot change simulation state.
    #[must_use]
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    /// Authoritative combat outcomes from the latest completed tick.
    pub fn defeats(&self) -> &[Defeat] {
        &self.defeats
    }

    /// Ticks completed since this world was created.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Replay fingerprint of authoritative state and contact counts.
    /// Contact counts detect broadphase changes that leave positions unchanged.
    /// Exhaustive destructuring requires field decisions, but tests must check that
    /// relevant changes actually affect the fingerprint.
    #[must_use]
    pub fn hash(&self) -> u64 {
        // Exhaustive on purpose — see above. Do not replace with `..`.
        let Self {
            bodies,
            tick,
            queue,
            sources,
            scenes,
            defeats,
            contacts,
            crowd_contacts,
            crowd_grid: _,
            material,
            trace,
        } = self;

        // Diagnostic retention must not affect the simulation fingerprint.
        let _ = trace;
        let Bodies {
            slots,
            physics,
            health,
            characters,
            interactions,
            pos: enemy_pos,
            prev_pos: enemy_prev,
        } = bodies;
        let mut h = Fnv::default();
        material.hash(&mut h);
        h.usize(defeats.len());
        for defeat in defeats {
            defeat.body.hash_into(&mut h);
            h.f32(defeat.position.x);
            h.f32(defeat.position.y);
            h.usize(usize::from(defeat.scene.is_some()));
            if let Some(scene) = defeat.scene {
                scene.hash_into(&mut h);
            }
            h.usize(usize::from(defeat.source.is_some()));
            if let Some(source) = defeat.source {
                source.hash_into(&mut h);
            }
        }
        physics.hash(&mut h);
        interactions.hash(&mut h);
        characters.hash(&mut h);

        h.u64(*tick);
        h.usize(*contacts);
        h.usize(*crowd_contacts);

        // Pending requests are state, and the kind a point sample is worst at:
        // two worlds identical in every body still diverge on the next tick if
        // one of them is about to grant a spawn the other is not.
        queue.hash(&mut h);

        // A source's countdown and emission count are what decide *when* and
        // *where* the next body appears, so two worlds identical in every body
        // diverge from here. Sources are also the one piece of state a replay
        // could otherwise agree on for a hundred ticks and then disagree about
        // all at once.
        sources.hash(&mut h);
        scenes.hash(&mut h);

        // The previous tick goes in too. It is derived — it is just last tick's
        // values — so it adds no information to a comparison of two runs, and
        // it is included anyway, because the rule is *every field* and an
        // exception is how that rule stops being checkable. The compile error
        // that brought you here is the guard working.

        // Identity, not just geometry. Two hordes standing in identical places
        // are still different worlds if their bodies have different names —
        // the next thing spawned gets a different id in each, and they diverge
        // for real a tick later. See `Slots::hash`.
        slots.hash(&mut h);
        health.hash(&mut h);

        // Length as well as contents: two hordes agreeing on every body they
        // share are still different worlds if one has more of them.
        h.usize(enemy_pos.len());
        for p in enemy_pos.iter().chain(enemy_prev) {
            h.f32(p.x);
            h.f32(p.y);
        }

        h.finish()
    }

    /// Where the character is standing, lifted to world space for the camera
    /// and the renderer. Stored on the ground plane; the height is a constant
    /// of the body's size rather than state.
    pub fn player_pos(&self) -> Vec3 {
        on_ground(self.bodies.pos[0], PLAYER_HALF_HEIGHT)
    }

    /// Immutable physical pose of the persistent player body.
    pub fn player_presentation(&self, alpha: Alpha) -> CharacterPresentation {
        CharacterPresentation::new(self.player_id(), on_ground(self.bodies.prev_pos[0].lerp(self.bodies.pos[0], alpha.get()), 0.0), self.bodies.pos[0] - self.bodies.prev_pos[0])
    }

    /// Immutable, allocation-free presentation facts for every character member.
    ///
    /// Defeated characters leave this iterator immediately; Game presents their corpses.
    /// Character membership is otherwise independent of health and body order.
    /// Dense rows may move after a despawn, so callers receive stable ids and
    /// must not treat iterator position as identity.
    pub fn character_presentations(
        &self,
        alpha: Alpha,
    ) -> impl Iterator<Item = CharacterPresentation> + '_ {
        let a = alpha.get();
        self.bodies.slots.ids()[1..]
            .iter()
            .copied()
            .enumerate()
            .filter_map(move |(offset, id)| {
                if !self.bodies.characters.contains(id) || self.bodies.health.get(id) == Some(0) {
                    return None;
                }
                let row = offset + 1;
                let position = self.bodies.prev_pos[row].lerp(self.bodies.pos[row], a);
                Some(CharacterPresentation::new(
                    id,
                    on_ground(position, 0.0),
                    self.bodies.pos[row] - self.bodies.prev_pos[row],
                ))
            })
    }

    /// Read-only collision discs at the last completed simulation tick.
    /// Includes the player, enemies and disc props; jointed bodies have their own
    /// geometry in [`World::joint_presentations`].
    pub fn collision_discs(&self) -> impl Iterator<Item = CollisionDisc> + '_ {
        self.bodies
            .slots
            .ids()
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(row, id)| {
                if self.bodies.physics.articulated(id) {
                    return None;
                }
                Some(CollisionDisc::new(
                    id,
                    on_ground(self.bodies.pos[row], 0.0),
                    body_radius(row),
                ))
            })
    }

    /// Immutable prop facts, excluding the player and character presentations.
    pub fn prop_presentations(&self, alpha: Alpha) -> impl Iterator<Item = PropPresentation> + '_ {
        self.bodies.slots.ids()[1..]
            .iter()
            .copied()
            .enumerate()
            .filter_map(move |(offset, id)| {
                if self.bodies.characters.contains(id) || self.bodies.physics.articulated(id) {
                    return None;
                }
                let row = offset + 1;
                let position = self.bodies.prev_pos[row].lerp(self.bodies.pos[row], alpha.get());
                Some(PropPresentation::new(
                    id,
                    on_ground(position, 0.0),
                    self.bodies.interactions.get(id),
                ))
            })
    }

    /// The player's stable identity, usable by every physical interaction.
    #[must_use]
    pub fn player_id(&self) -> EntityId {
        self.bodies.slots.ids()[0]
    }

    /// Resolves an observed name (or "player") without manufacturing an id.
    #[must_use]
    pub fn body_named(&self, name: &str) -> Option<EntityId> {
        if name == "player" {
            return Some(self.player_id());
        }
        self.bodies
            .slots
            .ids()
            .iter()
            .copied()
            .find(|id| id.to_string() == name)
    }

    /// Read-only physical state for a live body, independent of its behaviours.
    #[must_use]
    pub fn motion(&self, id: EntityId) -> Option<Motion> {
        self.bodies.physics.get(id)
    }

    /// Applies an external impulse now; its movement begins on the next tick.
    /// Stale identities are refused rather than affecting a recycled body.
    #[must_use]
    pub fn apply_impulse(&mut self, id: EntityId, impulse: Impulse) -> bool {
        self.bodies
            .physics
            .impulse(id, impulse, None, &mut self.trace.sink(self.tick))
    }

    /// Apply a world-space point impulse; stale bodies or nonfinite points refuse it.
    pub fn apply_impulse_at(&mut self, id: EntityId, point: Vec2, impulse: Impulse) -> bool {
        self.bodies
            .physics
            .impulse_at(id, point, impulse, None, &mut self.trace.sink(self.tick))
    }

    /// Current joint coordinate, lock and collider of a jointed body.
    pub fn joint(&self, id: EntityId) -> Option<JointState> {
        self.bodies.physics.joint(id)
    }

    /// Jointed bodies with interpolated poses, independent of disc props.
    pub fn joint_presentations(&self, alpha: Alpha) -> impl Iterator<Item = JointPresentation> + '_ {
        self.bodies.physics.joints.presentations(alpha)
    }

    /// One jointed body with an interpolated pose, or None for a stale identity.
    pub fn joint_presentation(&self, id: EntityId, alpha: Alpha) -> Option<JointPresentation> {
        self.bodies.physics.joint(id).map(|j| j.at(alpha))
    }

    /// Bodies met by moving jointed bodies in the latest tick, in order.
    /// Replaced by the next step; damage is applied through [`ResolvedStep`].
    pub fn impacts(&self) -> &[Impact] {
        &self.bodies.physics.joints.impacts
    }

    /// Player pairs overlapping at the start of the last step's contacts.
    pub fn contacts(&self) -> usize {
        self.contacts
    }

    /// Non-player pairs overlapping at the start of the last step's contacts.
    #[must_use]
    pub fn crowd_contacts(&self) -> usize {
        self.crowd_contacts
    }

    /// Whether all current positions are finite; tolerance comparisons alone miss NaN.
    #[must_use]
    pub fn all_positions_finite(&self) -> bool {
        self.bodies.physics.finite()
            && self.bodies.pos[0].is_finite()
            && self.bodies.prev_pos[0].is_finite()
            && self.bodies.pos.iter().all(|p| p.is_finite())
            && self.bodies.prev_pos.iter().all(|p| p.is_finite())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[global_allocator]
static COUNTING: tests::alloc_counter::Counting = tests::alloc_counter::Counting;

#[cfg(test)]
const TEST_CHARACTER: Template = Template::BODY
    .with_health(core::num::NonZeroU8::new(3).unwrap())
    .character();
