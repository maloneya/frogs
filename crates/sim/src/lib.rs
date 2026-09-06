//! What exists, and how it describes itself to a renderer.
//!
//! Depends on `arpg-core` for vocabulary and on nothing else. In particular it
//! does not link wgpu, so simulation tests run without a GPU.

use glam::{Vec2, Vec3};

use arpg_core::{Instance, InstanceSink, Intent, Report, MAX_INSTANCES};

mod angle;
mod contact;
mod hash;
mod members;
mod pass;
mod slots;
mod time;
mod trace;

pub use hash::Fnv;
/// What to make, and what behaviours it should be granted. See
/// [`crate::pass::spawn`] for why spawning is a queue rather than a call.
pub use pass::spawn::Template;
/// Who asks for spawns, and when. See [`crate::pass::source`] for why a source
/// is its own thing rather than a behaviour on a body.
pub use pass::source::{Condition, Placement, Source, SourceId};
pub use slots::EntityId;
use members::Members;
use pass::source::Sources;
use pass::spawn::SpawnQueue;
use slots::Slots;
pub use time::{Accumulator, Alpha, Dt, Ticks, TICK_HZ};
pub use trace::{Event, Trace};

/// One tick of walking, in world units. Re-exported because a scenario or a
/// harness reading a position is almost always trying to work out how many
/// ticks of movement it is looking at.
pub use pass::walk::PER_TICK as WALK_PER_TICK;

/// Blends between two facings the short way round.
///
/// A plain lerp is wrong here and wrong *invisibly*: `facing` is wrapped to
/// `-PI..=PI`, so a character turning through south goes from `3.13` to `-3.13`
/// in one tick — a real turn of 0.02 radians. Lerping those endpoints sends the
/// body spinning 6.26 radians the other way, across a single frame, for one
/// frame. It reads as a flicker rather than as a spin, which is exactly the
/// kind of artefact that gets blamed on the renderer.
///
/// `angle::shortest_arc` already solves this for the simulation's own turning;
/// this is the same fix applied to the drawing of it.
fn blend_angle(from: f32, to: f32, alpha: Alpha) -> f32 {
    angle::wrap(from + angle::shortest_arc(from, to) * alpha.get())
}

/// Lifts a ground-plane position into world space at a given height.
///
/// The one place `Vec2::y` is allowed to mean world **Z**. Spelling the swap
/// out once, rather than writing `Vec3::new(p.x, h, p.y)` at each call site,
/// is what keeps it from being a silent trap: the two axes are both horizontal
/// and both plausible, so a transposition compiles, draws, and puts everything
/// in the wrong place along one diagonal.
fn on_ground(p: Vec2, height: f32) -> Vec3 {
    Vec3::new(p.x, height, p.y)
}

/// Ground plane size, in tiles.
///
/// Sized so the world is comfortably larger than the view. A tracking camera is
/// meaningless otherwise: if the whole arena fits on screen there is nothing
/// for the camera to reveal, and following just slides the floor around inside
/// a frame that already showed everything.
///
/// It is also why the camera does *not* clamp itself to the world bounds. The
/// view covers ~57x55 world units of floor, a footprint of ~40 units either
/// side of the focus, so an arena has to be several times that before a clamp
/// leaves the camera any room to move at all. A bigger world is the fix a
/// camera clamp only pretends to be.
const GROUND_TILES: usize = 128;
const TILE: f32 = 1.5;

// Tuning constants are the one thing in this file with no type protecting them.
// They are bare numbers, and the plausible wrong edit — a negative speed, a turn
// rate of zero, a square footprint — produces silently wrong behaviour rather
// than an error. A const assert is the cheapest guard there is and it fails at
// compile time, so it belongs on every one of them.
const _: () = assert!(GROUND_TILES > 0);
const _: () = assert!(TILE > 0.0);

/// The floor's share of the instance budget.
const GROUND_INSTANCES: usize = GROUND_TILES * GROUND_TILES;
const _: () = assert!(GROUND_INSTANCES < MAX_INSTANCES, "the floor alone must fit the buffer");

/// How large the horde may grow. The ground and the player are drawn from the
/// same instance buffer in the same draw call, so the enemy budget is whatever
/// they leave behind.
///
/// It lives next to the field it bounds rather than in the caller, so `World`
/// knows its own limit. A second writer of the horde that had to remember the
/// subtraction would silently overrun the GPU buffer — a failure with no error
/// message, since the upload just truncates.
const MAX_ENEMIES: usize = MAX_INSTANCES - GROUND_INSTANCES - 1;

/// How large the horde starts. Big enough to read as a crowd, small enough that
/// the brute-force passes landing next stay comfortably inside a frame.
const DEFAULT_ENEMIES: usize = 1024;
const _: () = assert!(DEFAULT_ENEMIES >= 1 && DEFAULT_ENEMIES <= MAX_ENEMIES);

/// One enemy, as drawn. Uniform on purpose: a horde of identically sized bodies
/// is what lets the broadphase be a flat uniform grid rather than a hierarchy,
/// since every structure that beats a grid does so by adapting to size variance
/// there is none of here.
const ENEMY_SCALE: Vec3 = Vec3::splat(0.5);
const _: () = assert!(ENEMY_SCALE.x > 0.0 && ENEMY_SCALE.y > 0.0 && ENEMY_SCALE.z > 0.0);

/// Where an enemy's centre sits so the cube rests *on* the floor rather than
/// half sunk into it. Derived rather than written down, so the two cannot
/// disagree after someone resizes the body.
const ENEMY_HALF_HEIGHT: f32 = ENEMY_SCALE.y * 0.5;

/// Spacing of the spawn grid, in world units.
///
/// Deliberately *not* `TILE`. Matching the floor's spacing made the horde tile
/// it exactly edge-to-edge, hiding the ground and — worse — making a change in
/// N invisible, because the horde only grew off-screen.
///
/// It is also wider than the body, and that is about to start mattering: once
/// overlapping bodies push each other apart, a horde that spawns already
/// interpenetrated resolves all of it on the first frame and detonates. The
/// gap is the difference between a crowd and an explosion, so it is a compile
/// error to close it rather than a comment someone might read.
const ENEMY_SPACING: f32 = 0.7;
const _: () =
    assert!(ENEMY_SPACING > ENEMY_SCALE.x, "a horde that spawns overlapped blows itself apart");

/// Collision radii, in world units. **Bodies are discs, not boxes**, and that
/// is a decision the camera pays for rather than a shortcut.
///
/// A disc is rotation-invariant, which matters because the player turns
/// continuously: a box would need its collider rebuilt every frame, and its
/// minimum-translation axis *flips* as two boxes slide past each other, which
/// is a documented source of crowd jitter. A disc has one contact normal and
/// one penetration depth, both unambiguous, and the test is a squared-distance
/// compare with no `sqrt` until an overlap is confirmed.
///
/// What makes it *free* rather than merely cheap is the projection. A sorted-2D
/// isometric game has to prevent overlap or the sort order pops, so the
/// renderer dictates the radius. Here the depth buffer resolves occlusion
/// exactly in hardware, so bodies may interpenetrate and the image stays
/// correct — which leaves the radius as a pure feel knob, answerable to how
/// dense the crowd should be and to nothing else.
const ENEMY_RADIUS: f32 = ENEMY_SCALE.x * 0.5;

/// Between the player's half-width (0.225) and half-depth (0.4), since one
/// circle has to stand in for a footprint that is deliberately not square.
const PLAYER_RADIUS: f32 = 0.3;

const _: () = assert!(ENEMY_RADIUS > 0.0 && PLAYER_RADIUS > 0.0);
const _: () = assert!(
    ENEMY_SPACING > 2.0 * ENEMY_RADIUS,
    "bodies must spawn clear of each other, not merely with their cubes apart"
);

/// Half the ground plane's width, in world units.
const ARENA_HALF: f32 = GROUND_TILES as f32 * TILE * 0.5;
const _: () = assert!(ARENA_HALF > PLAYER_SCALE.x && ARENA_HALF > PLAYER_SCALE.z);

/// Deliberately taller than an enemy (0.5), so the player stays readable from
/// inside a crowd of them. Silhouette is the cheapest legibility tool there is.
///
/// Also deliberately *not* square in plan: deeper along its own +Z (the facing
/// axis) than it is wide. A square footprint rotated about the vertical axis
/// looks almost identical at every angle, so the character would turn correctly
/// and appear not to — the facing would be real but invisible.
const PLAYER_SCALE: Vec3 = Vec3::new(0.45, 1.2, 0.8);
const _: () = assert!(PLAYER_SCALE.x > 0.0 && PLAYER_SCALE.y > 0.0 && PLAYER_SCALE.z > 0.0);
// The reason the body is not square, promoted from a comment to a compile
// error: a square footprint turns correctly and looks identical at every angle,
// so the facing would be real and invisible.
const _: () = assert!(PLAYER_SCALE.x != PLAYER_SCALE.z, "a square footprint makes facing invisible");

/// Where the player's centre sits so the body rests on the floor. Derived from
/// the scale for the same reason the enemy's is: two numbers that must agree
/// should be one number.
const PLAYER_HALF_HEIGHT: f32 = PLAYER_SCALE.y * 0.5;

/// Where the horde is.
///
/// Structure-of-arrays rather than `Vec<Enemy>`. The solvers walk positions and
/// nothing else, and a contiguous stream is what they want; fields an enemy
/// gains later — health, AI state, cooldowns — belong in their own arrays
/// beside this one, so the hot loop never drags them through cache on its way
/// to a position it does want.
#[derive(Default)]
struct Enemies {
    /// Stable names for these bodies, and the map onto the dense rows below.
    ///
    /// The horde stays dense — the passes want a contiguous stream — so a
    /// body's row moves whenever anything before it dies. This is what lets
    /// something hold a reference across that: see [`crate::slots`].
    slots: Slots,
    /// Ground-plane position: `x` is world X and `y` is world **Z** — see
    /// [`on_ground`], which is the only place that swap is spelled out.
    ///
    /// `Vec2` rather than `Vec3` because the horde never leaves the floor: the
    /// height a body is drawn at is a constant of its size, not state worth
    /// storing N times. That is what makes collision here genuinely 2D, which
    /// is the largest saving the isometric camera hands over — a grid
    /// neighbourhood is 9 cells rather than 27, and with no vertical axis there
    /// is no stacking, which is the case that forces general solvers into four
    /// to eight iterations.
    pos: Vec<Vec2>,
    /// Where each body was at the end of the *previous* tick.
    ///
    /// Read only by `extract`, written only by [`crate::pass::remember`]. It is
    /// simulation-owned data that exists purely for presentation, which sounds
    /// like a contradiction and is not: the alternative is `app` snapshotting a
    /// thousand positions every tick to hand back later — the same copy, done
    /// further from the data and with a chance of being skipped.
    ///
    /// Always exactly as long as `pos`; see [`Enemies::debug_check_paired`].
    prev_pos: Vec<Vec2>,
}

impl Enemies {
    fn len(&self) -> usize {
        self.pos.len()
    }

    /// Lays `n` enemies out in a square grid centred on the origin.
    ///
    /// Respawns the whole horde rather than appending to it, so the layout
    /// stays a function of N alone: halving it re-centres what is left rather
    /// than deleting a corner. Bodies that persist across a count change is the
    /// better model, and it belongs to the real spawner (roadmap chunk 7)
    /// rather than to a debug dial.
    fn respawn(&mut self, n: usize) {
        // `clear` keeps the allocations, so doubling N repeatedly grows the
        // buffers a few times rather than reallocating on every press.
        self.clear();
        self.pos.reserve(n);
        self.prev_pos.reserve(n);

        let side = (n as f32).sqrt().ceil().max(1.0) as usize;
        let offset = (side as f32 - 1.0) * ENEMY_SPACING * 0.5;

        for i in 0..n {
            self.spawn(Vec2::new(
                (i % side) as f32 * ENEMY_SPACING - offset,
                (i / side) as f32 * ENEMY_SPACING - offset,
            ));
        }
    }

    /// Empties the horde, retiring every name. Ids from before it stay dead —
    /// see [`Slots::clear`].
    fn clear(&mut self) {
        self.slots.clear();
        self.pos.clear();
        self.prev_pos.clear();
    }

    /// Puts one body at a chosen place and returns its name.
    ///
    /// **`prev_pos` is seeded to `at` here, and that is the point of routing
    /// every spawn through one function.** A body whose previous position is
    /// wherever the array happened to hold gets drawn streaking across the
    /// arena for exactly one frame — invisible under vsync, where nearly every
    /// frame runs a tick, and obvious uncapped, where most frames run none.
    /// `docs/traps.md` carries that symptom; maintaining the invariant in the
    /// storage is what retires it, rather than testing each caller for it.
    fn spawn(&mut self, at: Vec2) -> EntityId {
        let id = self.slots.insert();
        self.pos.push(at);
        self.prev_pos.push(at);
        self.debug_check_paired();
        id
    }

    /// The pairing `Slots` documents but cannot check: it holds no payload, so
    /// keeping every array the same length is this type's job. Debug only —
    /// it is a claim about this code rather than about the world, and an array
    /// added later and forgotten in `despawn` is the bug it catches.
    fn debug_check_paired(&self) {
        debug_assert_eq!(self.slots.len(), self.pos.len(), "slots and pos disagree");
        debug_assert_eq!(self.slots.len(), self.prev_pos.len(), "slots and prev_pos disagree");
    }

    /// Removes one body. Returns whether the id named a live one.
    ///
    /// Every parallel array is `swap_remove`d at the index `Slots` hands back,
    /// which keeps the rows dense and the correspondence intact. An array added
    /// later and forgotten here is the one bug this shape still allows, and
    /// [`Enemies::debug_check_paired`] is what catches it.
    fn despawn(&mut self, id: EntityId) -> bool {
        let Some(dense) = self.slots.remove(id) else { return false };

        self.pos.swap_remove(dense);
        self.prev_pos.swap_remove(dense);
        self.debug_check_paired();
        true
    }

    /// Where a named body stands, or `None` if it is dead.
    fn pos_of(&self, id: EntityId) -> Option<Vec2> {
        self.slots.index(id).map(|i| self.pos[i])
    }
}

/// The player-controlled character.
///
/// A single struct held apart from the horde, and **that is a limitation rather
/// than a design**. The original argument for it — that player-only fields
/// would cost N times if the player were a row — dissolved when behaviours
/// became sparse sets: state only the player has lives in its own membership
/// set, so the player can be a body without any enemy paying for it. What
/// remains is that the passes take its fields individually where the horde gets
/// a slice, so the borrow checker checks half of every signature. Roadmap
/// chunk 4 closes that.
#[derive(Default)]
struct Player {
    /// Ground-plane position, on the same terms as the horde's: the height a
    /// body is drawn at is a constant of its size, so there is no Y here for
    /// movement to leave the floor through. Unrepresentable rather than tested,
    /// which is where an invariant belongs.
    pos: Vec2,
    /// Which way the body points, in radians. Yaw 0 faces world +Z and positive
    /// turns toward +X, matching `Instance::with_yaw` and `shader.wgsl`.
    ///
    /// Simulation state, not a rendering detail: this is what the attack hitbox
    /// will be oriented by, so it has to be something the sim owns and can be
    /// reasoned about without a GPU.
    facing: f32,
    /// The pair above as they stood at the end of the previous tick. See
    /// `Enemies::prev_pos`.
    prev_pos: Vec2,
    prev_facing: f32,
    /// Where the swing has got to.
    ///
    /// On the player rather than in a membership set, and this is the case the
    /// sparse-set argument does *not* apply to: there is exactly one of these,
    /// so a set keyed by entity would be a table with one row and a lookup to
    /// find it. Behaviours are for what *some* bodies have; this is what the
    /// one player has.
    attack: pass::attack::Attack,
}

/// What exists: the horde, the player, and the bookkeeping the two seams —
/// [`World::extract`] and [`World::trace`] — hand out.
pub struct World {
    enemies: Enemies,
    player: Player,
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
    /// Player-against-horde contacts resolved by the last [`World::step`].
    ///
    /// The instrument the collision work is measured with. A body is nine
    /// pixels across at this zoom and a contact displaces it by a fraction of
    /// that, so "is anything actually touching" is invisible on screen and
    /// obvious as a number. It earns its keep again when the broadphase lands:
    /// a grid that finds a different number of pairs than brute force is wrong,
    /// and this is what catches it.
    contacts: usize,
    /// Which bodies chase the player.
    ///
    /// **A behaviour, stored as its own membership set rather than as a field
    /// on every body.** A body is not "an enemy with `chases: bool`"; it is a
    /// body, and it is in this set or it is not. Adding the next behaviour adds
    /// another field here and another line to the schedule, and touches nothing
    /// that already exists — which is the property the storage was chosen for.
    ///
    /// See [`crate::members`] for why the set is sparse, and
    /// [`crate::pass::seek`] for the pass that walks it.
    seekers: Members,
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
    /// Enemy pairs the crowd solver pushed apart in the last [`World::step`].
    ///
    /// Kept apart from `contacts` rather than summed into it: one number
    /// covering both cannot distinguish the player wading into a pack from the
    /// pack settling on its own, and it is the crowd half specifically that the
    /// uniform grid will change.
    crowd_contacts: usize,
}

impl Default for World {
    fn default() -> Self {
        let mut world = Self {
            enemies: Enemies::default(),
            player: Player::default(),
            tick: 0,
            trace: Trace::default(),
            seekers: Members::default(),
            queue: SpawnQueue::default(),
            sources: Sources::default(),
            contacts: 0,
            crowd_contacts: 0,
        };
        world.set_enemy_count(DEFAULT_ENEMIES);
        world
    }
}

impl World {
    /// How many enemies the horde currently holds. Always within
    /// `1..=MAX_ENEMIES`, because [`World::set_enemy_count`] is the only writer.
    ///
    /// Derived from the storage rather than tracked beside it: a separate
    /// counter is a second copy of the same fact, and the two drift the first
    /// time something spawns or kills one without going through the dial.
    pub fn enemy_count(&self) -> usize {
        self.enemies.len()
    }

    /// The only door in, so the clamp cannot be bypassed or forgotten. A
    /// spawner, a save-load path or a debug console added later inherits it
    /// without having to know `MAX_ENEMIES` exists.
    ///
    /// **Zero is allowed.** An empty arena is a state this game reaches by
    /// playing it well, not an error. It is also what makes the simplest
    /// scenarios writable: the horde and the player both spawn centred on the
    /// origin, so *any* horde puts the two in contact on tick zero, and a
    /// prediction about plain movement becomes a prediction about the solver.
    pub fn set_enemy_count(&mut self, n: usize) {
        // **Exhaustive, for the reason `hash` and `report` are.** This is one of
        // the two doors where a behaviour set must be considered, so a new field
        // stops the crate compiling until someone has decided here whether a
        // wholesale respawn should revoke it.
        // `sources` is deliberately untouched. This dial resizes the *horde*,
        // and a source is not a body: clearing the level's sources because
        // somebody asked for a different number of enemies would be a debug
        // key deleting content.
        let Self {
            enemies,
            seekers,
            queue,
            trace,
            tick,
            sources: _,
            player: _,
            contacts: _,
            crowd_contacts: _,
        } = self;

        enemies.respawn(n.min(MAX_ENEMIES));

        // Pending requests are decisions made *before* this reset, and granting
        // them afterwards would put bodies in an arena that was just rebuilt to
        // hold a stated number. The dial is a reset; this is part of resetting.
        queue.clear();

        // Every name the horde had is now retired, so a set that survived would
        // hold only ids that resolve to nothing. Cleared anyway, because the
        // alternative is a set that grows by the size of the old horde on every
        // respawn and costs every pass that walks it.
        seekers.clear();

        // Traced because it happens *outside* the schedule. State that changes
        // between ticks is the hardest kind to account for later, so it is
        // exactly what the trace is for. Stamped with the tick it precedes.
        trace.sink(*tick).emit(Event::Spawned { count: enemies.len() });
    }

    /// Places one body at a chosen spot and returns its name.
    ///
    /// `at` is a **ground-plane** position: `x` is world X and `y` is world Z,
    /// the same convention the storage uses. See [`on_ground`], the one place
    /// that swap is spelled out.
    ///
    /// Returns `None` when the horde is already at [`MAX_ENEMIES`]. That is a
    /// refusal rather than a clamp because the budget exists to stop the
    /// instance buffer overrunning, and an overrun is silent — the upload just
    /// truncates, so bodies stop being drawn with no error anywhere. A caller
    /// that ignores this gets a compiler warning; a caller that never saw it
    /// would get an invisible bug.
    ///
    /// **The door scenarios place bodies through.** A horde count lays N bodies
    /// out in a grid nobody wrote down; this is what lets an assertion be about
    /// a body at a *particular* spot.
    ///
    /// **Immediate, which is what makes it a setup door rather than a gameplay
    /// one.** Nothing inside the simulation can call this, and that is a fact
    /// rather than a rule: a spawn during a tick moves rows out from under
    /// indices the passes have already taken, and no pass is ever handed a
    /// `&mut World` to reach it with. Setup and the harness may, because
    /// neither runs while the schedule does. Everything else asks — see
    /// [`World::request_spawn`].
    pub fn place(&mut self, at: Vec2, what: Template) -> Option<EntityId> {
        // Traced for the same reason `set_enemy_count` is: it happens *outside*
        // the schedule, and state that changes between ticks is the hardest kind
        // to account for when reading a trace later. The event is emitted by
        // `pass::spawn::place`, so both doors record it identically. The bulk
        // path does not come through here — `respawn` writes the storage
        // directly.
        let mut trace = self.trace.sink(self.tick);
        pass::spawn::place(&mut self.enemies, &mut self.seekers, at, what, &mut trace)
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
        self.trace.sink(self.tick).emit(Event::SourceRemoved { id });
        true
    }

    /// How many sources are live.
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Asks for something to be made. It exists at the end of the next tick to
    /// run.
    ///
    /// **The door for anything that runs while the simulation does**, and the
    /// only one that is safe there: this touches no storage, so it cannot move
    /// a row under a pass that is mid-iteration. The request is granted by
    /// `pass::spawn::drain`, first in the next schedule.
    ///
    /// Returns `false` when the queue is full, in which case the request is
    /// dropped and counted — a `Refused` trace event follows on the next tick.
    /// A caller that ignores this gets a warning; a caller that never saw it
    /// would get a body that silently never appears.
    #[must_use]
    pub fn request_spawn(&mut self, at: Vec2, what: Template) -> bool {
        self.queue.push(at, what)
    }

    /// Removes one body. Returns whether the id named a live one.
    ///
    /// A stale id is a no-op rather than an error: something holding a
    /// reference to a body that has already died is the normal case, not a
    /// mistake, and it is exactly what [`EntityId`]'s generation makes safe to
    /// ask about.
    pub fn despawn_enemy(&mut self, id: EntityId) -> bool {
        // Exhaustive for the same reason as `set_enemy_count`: the other door a
        // new behaviour has to be considered at. A line per behaviour rather
        // than a registry is deliberate — a list of sets to sweep would be a
        // second hand-maintained record of which behaviours exist.
        // `queue` is deliberately untouched: a request names a place and a
        // template, never a body, so nothing pending can refer to the name
        // being retired here.
        let Self {
            enemies,
            seekers,
            trace,
            tick,
            queue: _,
            sources: _,
            player: _,
            contacts: _,
            crowd_contacts: _,
        } = self;

        if !enemies.despawn(id) {
            return false;
        }

        // Revoking eagerly is an optimisation, not a correctness requirement: a
        // stale id resolves to nothing, which is what generational ids are for,
        // and `pass::seek` skips what it cannot resolve. This keeps the sets
        // from accumulating garbage that costs every pass that walks them.
        seekers.remove(id);

        trace.sink(*tick).emit(Event::Removed { id });
        true
    }

    /// Makes a body chase the player. Returns whether it took effect.
    ///
    /// `false` when the id is dead or the body already chases. Composing rather
    /// than configuring: a body is placed first and granted behaviours after,
    /// so what an enemy *is* stays a list of the things it does.
    pub fn add_seek(&mut self, id: EntityId) -> bool {
        if !self.enemies.slots.contains(id) {
            return false;
        }
        self.seekers.add(id).is_some()
    }

    /// Whether this body chases the player.
    #[must_use]
    pub fn is_seeker(&self, id: EntityId) -> bool {
        self.seekers.contains(id)
    }

    /// Makes the first `n` bodies chase, and the rest not.
    ///
    /// **A debug dial, and shaped like one.** It exists so the behaviour can be
    /// turned on in the running game without a rebuild, the way
    /// [`World::set_enemy_count`] can. It is not how a real spawner should work
    /// — that will grant behaviours per body as it places them, from something
    /// describing what *kind* of enemy this is — and the giveaway is "the first
    /// n", which is a fact about storage order rather than about the game.
    pub fn set_seeker_count(&mut self, n: usize) {
        self.seekers.clear();
        for id in self.enemies.slots.ids().iter().take(n) {
            self.seekers.add(*id);
        }
    }

    /// Whether the hitbox exists right now.
    #[must_use]
    pub fn hitbox_is_live(&self) -> bool {
        self.player.attack.hitbox_is_live()
    }

    /// How many bodies the current or most recent swing struck.
    #[must_use]
    pub fn struck(&self) -> usize {
        self.player.attack.struck()
    }

    /// How many bodies chase the player.
    #[must_use]
    pub fn seeker_count(&self) -> usize {
        self.seekers.len()
    }

    /// Where a named body stands, lifted to world space, or `None` if it is
    /// dead. The height is a constant of the body's size rather than state —
    /// see [`Enemies::pos`].
    #[must_use]
    pub fn enemy_pos(&self, id: EntityId) -> Option<Vec3> {
        self.enemies.pos_of(id).map(|p| on_ground(p, ENEMY_HALF_HEIGHT))
    }

    /// Whether this id still names a live body.
    #[must_use]
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.enemies.slots.contains(id)
    }

    /// Describes itself for an agent, field by field.
    ///
    /// **The same exhaustive destructuring as [`World::hash`], for the same
    /// reason.** A field added to `World` fails to compile until it is
    /// reported, so "anything an agent must observe is a derived field" is a
    /// rule the compiler applies rather than one someone remembers.
    pub fn report(&self, out: &mut Report) {
        let Self { enemies, player, tick, seekers, queue, sources, contacts, crowd_contacts, trace } =
            self;
        let Player { pos, facing, prev_pos, prev_facing, attack } = player;

        out.int("tick", *tick);
        out.vec3("player_pos", on_ground(*pos, PLAYER_HALF_HEIGHT));
        out.num("facing", *facing);
        out.int("contacts", *contacts as u64);
        out.int("crowd_contacts", *crowd_contacts as u64);

        // Derived, and reported because "everything is still a number" is not
        // visible in any of the values above — a NaN position prints as a
        // position and compares equal to nothing.
        out.bool("finite", self.all_positions_finite());
        out.int("enemies", enemies.len() as u64);
        out.int("seekers", seekers.len() as u64);

        // Asked for and not yet made. Without it, "the spawn has not happened
        // yet" and "the spawn was refused" look identical from out here — and
        // one of those is a bug.
        out.int("queued", queue.len() as u64);
        out.int("sources", sources.len() as u64);

        // The swing, as three derived facts. `state` is a point sample and
        // cannot show a window, so these say where in the window the sample
        // fell; `trace` is what shows the window itself.
        out.bool("swinging", attack.is_swinging());
        out.int("swing_tick", u64::from(attack.elapsed()));
        out.bool("hitbox", attack.hitbox_is_live());
        out.int("struck", attack.struck() as u64);
        out.int("trace_events", trace.len() as u64);
        out.int("trace_dropped", trace.dropped() as u64);

        // Previous-tick state is what render interpolation blends from. Not
        // interesting most of the time, and exactly the thing to look at when
        // something on screen is a tick behind where it should be.
        out.vec3("player_prev_pos", on_ground(*prev_pos, PLAYER_HALF_HEIGHT));
        out.num("prev_facing", *prev_facing);
    }

    /// Discards the trace so far. See [`Trace::clear`] for when that is right.
    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    /// What the simulation has done recently, oldest first.
    ///
    /// Read-only, like [`World::extract`]: perception must never be able to
    /// change what it observes.
    #[must_use]
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    /// Advances the world by `dt` seconds.
    ///
    /// `dt` is the raw frame time and movement is integrated against it, so
    /// speed is frame-rate independent today.
    ///
    /// `dt` is a [`Dt`], which carries no number and can only have come from an
    /// [`Accumulator`]. That is what makes "the simulation advances in fixed
    /// steps" checkable by the compiler rather than by review: there is no
    /// value a caller could pass to make this integrate a frame's worth of wall
    /// clock, because the type has no room to hold one.
    ///
    /// `intent` is the simulation's own vocabulary: a world-space direction and
    /// the discrete things asked for this tick. It is deliberately not
    /// `Actions`, which names its directions in *screen* space — resolving
    /// those is the camera's job, and letting it into `sim` would put a
    /// presentation decision inside the simulation.
    pub fn step(&mut self, dt: Dt, intent: Intent) {
        let move_dir = intent.move_dir();
        // Bound to the tick being run, so no pass can stamp an event with the
        // wrong one. See `trace::TraceSink`.
        let mut trace = self.trace.sink(self.tick);

        // **The schedule.** This function is a list of passes and nothing else:
        // no logic, no inline stages. Anything that reads like a step of the
        // simulation belongs in `pass/`, so that the order stays something you
        // can read in one screen and each pass's inputs stay visible in its
        // signature. `pass/mod.rs` carries why each adjacency is what it is.
        // **Decide, then perform.** `trigger` reads two facts about the world
        // and may only push onto the queue — it is handed no storage, so the
        // code that decides new bodies exist cannot make one. `drain` is the
        // only pass that changes what exists, and running it here means nothing
        // below has to defend against the horde changing length or moving rows
        // underneath it. See `pass::source` and `pass::spawn`.
        pass::source::trigger(
            &mut self.sources,
            &mut self.queue,
            self.player.pos,
            self.enemies.len(),
            trace.reborrow(),
        );
        pass::spawn::drain(
            &mut self.queue,
            &mut self.enemies,
            &mut self.seekers,
            trace.reborrow(),
        );
        pass::remember::remember(
            self.player.pos,
            self.player.facing,
            &mut self.player.prev_pos,
            &mut self.player.prev_facing,
            &self.enemies.pos,
            &mut self.enemies.prev_pos,
        );
        pass::walk::walk(&mut self.player.pos, move_dir, dt);
        // After `walk`, so chasers steer at where the player is *now* rather
        // than where it stood at the start of the tick. Before the solvers, so
        // the pile-up that chasing creates is what they resolve.
        pass::seek::seek(
            &self.seekers,
            &self.enemies.slots,
            &mut self.enemies.pos,
            self.player.pos,
            dt,
        );

        // Crowd before player, so the player's correction is the one that
        // survives the tick. `pass::separate` carries the argument.
        self.crowd_contacts = pass::separate::crowd(&mut self.enemies.pos, trace.reborrow());
        self.contacts = pass::separate::player(
            &mut self.player.pos,
            &mut self.enemies.pos,
            trace.reborrow(),
        );
        pass::contain::contain(&mut self.player.pos, &mut self.enemies.pos, trace.reborrow());
        pass::face::face(&mut self.player.facing, move_dir, dt);

        // **Last, and after `face`.** The hitbox is oriented by the facing this
        // tick ended with, and it is tested against where the bodies actually
        // ended up — after seeking, after both solvers, after the wall. Running
        // it earlier would swing at positions that no longer exist by the time
        // the tick is over.
        pass::attack::attack(
            &mut self.player.attack,
            self.player.pos,
            self.player.facing,
            intent.attack(),
            &self.enemies.slots,
            &self.enemies.pos,
            trace.reborrow(),
        );

        // Last, so that during the passes above `self.tick` names the tick being
        // run and afterwards it names how many have finished. Trace events are
        // stamped from inside a pass, so which of the two this is has to be
        // decided once and written down rather than rediscovered per pass.
        self.tick += 1;
    }

    /// Ticks completed since this world was created.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// A stable hash of **all** simulation state.
    ///
    /// Sampled every tick, two runs' hash sequences say which tick they
    /// diverged on rather than merely that they did. That is the whole
    /// instrument: `sim` is claimed to be a pure function of (state, inputs),
    /// and this is the thing that can exit nonzero when it stops being one.
    ///
    /// **Every field of `World` must be fed to this**, and the destructuring is
    /// what makes that a compile error rather than a rule someone remembers.
    /// Add a field and this stops building until it is hashed; leave it out and
    /// the replay gate reports a blind spot as agreement, because the failure
    /// mode of an incomplete hash is silence rather than noise. That is layer 1
    /// of the ladder in `CLAUDE.md`, where prose would have been layer 4.
    ///
    /// `contacts` goes in even though it is derived from the positions, and for
    /// a specific reason: a broadphase that finds a different number of pairs
    /// while leaving every body in the same place is exactly the bug the
    /// uniform grid will introduce, and positions alone would call it agreement.
    #[must_use]
    pub fn hash(&self) -> u64 {
        // Exhaustive on purpose — see above. Do not replace with `..`.
        let Self { enemies, player, tick, seekers, queue, sources, contacts, crowd_contacts, trace } =
            self;

        // **Deliberately not hashed**, and the exhaustive destructuring above is
        // what forced this line to be written rather than forgotten. The trace
        // is derived output — a record of what the passes did — so feeding it
        // back in would be hashing the hash's own inputs twice. It is also
        // bounded and wraps, which would make two runs of different lengths
        // disagree for a reason that has nothing to do with the simulation.
        let _ = trace;
        let Player { pos, facing, prev_pos, prev_facing, attack } = player;
        let Enemies { slots, pos: enemy_pos, prev_pos: enemy_prev } = enemies;

        let mut h = Fnv::default();

        h.u64(*tick);
        h.f32(pos.x);
        h.f32(pos.y);
        h.f32(*facing);
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

        // Membership is state. Two worlds whose bodies stand in identical
        // places are different worlds if one of them chases and the other does
        // not, and they diverge visibly on the very next tick.
        seekers.hash(&mut h);

        // The swing is simulation state with a *window*, which is exactly the
        // kind a point-sample hash is worst at describing — but leaving it out
        // would let a replay diverge on attack timing and call it agreement.
        attack.hash(&mut h);

        // The previous tick goes in too. It is derived — it is just last tick's
        // values — so it adds no information to a comparison of two runs, and
        // it is included anyway, because the rule is *every field* and an
        // exception is how that rule stops being checkable. The compile error
        // that brought you here is the guard working.
        h.f32(prev_pos.x);
        h.f32(prev_pos.y);
        h.f32(*prev_facing);

        // Identity, not just geometry. Two hordes standing in identical places
        // are still different worlds if their bodies have different names —
        // the next thing spawned gets a different id in each, and they diverge
        // for real a tick later. See `Slots::hash`.
        slots.hash(&mut h);

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
        on_ground(self.player.pos, PLAYER_HALF_HEIGHT)
    }

    /// Where the character should be *drawn* this frame, blended between the
    /// last two ticks.
    ///
    /// Separate from [`World::player_pos`] rather than replacing it, because
    /// the two answer different questions and confusing them is how
    /// presentation leaks into simulation. `player_pos` is where the character
    /// **is** — what a hitbox is tested against, what a scenario asserts on,
    /// what the harness reports. This is where it *appears*, which is a
    /// fractional tick behind and is nobody's business but the renderer's and
    /// the camera's.
    #[must_use]
    pub fn player_pos_at(&self, alpha: Alpha) -> Vec3 {
        on_ground(self.player.prev_pos.lerp(self.player.pos, alpha.get()), PLAYER_HALF_HEIGHT)
    }

    /// Which way the character is pointing, in radians. The attack state
    /// machine will orient its hitbox by this.
    pub fn player_facing(&self) -> f32 {
        self.player.facing
    }

    /// How many overlapping pairs the last step pushed apart, player against
    /// horde.
    pub fn contacts(&self) -> usize {
        self.contacts
    }

    /// How many overlapping enemy pairs the last step pushed apart.
    #[must_use]
    pub fn crowd_contacts(&self) -> usize {
        self.crowd_contacts
    }

    /// Whether every position in the world is a real number.
    ///
    /// **A poisoned position is the one failure that reads as success.** NaN
    /// propagates through arithmetic silently, survives `clamp` — which is what
    /// `pass::contain` would otherwise be expected to catch it with — and, worst
    /// of all, compares `false` to everything. A test written as
    /// `if (got - want).length() > tolerance` therefore *passes* when `got` is
    /// NaN, because `NaN > tolerance` is false. Every positional assertion in
    /// the scenario runner had that shape.
    ///
    /// So the check has to be an explicit "is this finite", asked separately,
    /// and it cannot be folded into a comparison. The solver is the thing that
    /// produces one: normalising the difference between two coincident bodies
    /// is a division by zero, which is why `contact::between` has a coincident
    /// case at all.
    #[must_use]
    pub fn all_positions_finite(&self) -> bool {
        self.player.pos.is_finite()
            && self.player.prev_pos.is_finite()
            && self.enemies.pos.iter().all(|p| p.is_finite())
            && self.enemies.prev_pos.iter().all(|p| p.is_finite())
    }

    /// **The seam.** The world describes itself in the renderer's vocabulary;
    /// `gfx` never sees a `World`.
    ///
    /// Takes the sink by value, so it is single-use and cannot outlive the
    /// frame. Everything about the buffer — that it was reset, that it is
    /// capacity-bounded, that pushing is the only thing anyone may do to it —
    /// is settled by the type rather than by remembering.
    ///
    /// `alpha` blends between the last two ticks. **This method takes `&self`,
    /// and that is the entire enforcement of "interpolation must never reach
    /// sim state"** — there is no `&mut` here for a blended value to be written
    /// back through, so the rule is layer 0 rather than a paragraph someone
    /// reads at session start.
    pub fn extract(&self, alpha: Alpha, mut out: InstanceSink<'_>) {
        self.extract_ground(&mut out);
        self.extract_enemies(alpha, &mut out);
        self.extract_player(alpha, &mut out);
    }

    /// The player is not a special case to the renderer either — one more cube
    /// in the same draw call. Only the colour and the silhouette distinguish it.
    fn extract_player(&self, alpha: Alpha, out: &mut InstanceSink<'_>) {
        // Linear, and it looks wrong here on purpose: the surface is sRGB, so
        // the hardware encodes on write. This is roughly sRGB (0.35, 0.72, 0.95)
        // — a bright cyan-blue, chosen to sit opposite the horde's muted red on
        // the colour wheel so the eye separates them without effort.
        out.push(
            Instance::new(self.player_pos_at(alpha), PLAYER_SCALE, Vec3::new(0.10, 0.47, 0.88))
                .with_yaw(blend_angle(self.player.prev_facing, self.player.facing, alpha)),
        );
    }

    /// The floor is not a special case — it is just more cube instances, flat
    /// and tinted. Same mesh, same pipeline, same draw call as the horde.
    fn extract_ground(&self, out: &mut InstanceSink<'_>) {
        let offset = (GROUND_TILES as f32 - 1.0) * TILE * 0.5;

        for z in 0..GROUND_TILES {
            for x in 0..GROUND_TILES {
                let checker = (x + z) % 2 == 0;
                let shade = if checker { 0.022 } else { 0.038 };
                out.push(Instance::new(
                    Vec3::new(x as f32 * TILE - offset, -0.05, z as f32 * TILE - offset),
                    Vec3::new(TILE, 0.1, TILE),
                    Vec3::new(shade, shade * 1.05, shade * 1.25),
                ));
            }
        }
    }

    /// Draws the horde from its stored positions, blended between the last two
    /// ticks. What is drawn is what the simulation believes.
    fn extract_enemies(&self, alpha: Alpha, out: &mut InstanceSink<'_>) {
        let a = alpha.get();

        for (i, (&pos, &prev)) in self.enemies.pos.iter().zip(&self.enemies.prev_pos).enumerate() {
            // Linear-space colour, since the surface is sRGB and the hardware
            // encodes on write. These look darker here than they will on screen.
            let t = (i % 7) as f32 / 7.0;
            let color = Vec3::new(0.30 + t * 0.12, 0.06 + t * 0.05, 0.05);

            let drawn = prev.lerp(pos, a);
            out.push(Instance::new(on_ground(drawn, ENEMY_HALF_HEIGHT), ENEMY_SCALE, color));
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[global_allocator]
static COUNTING: tests::alloc_counter::Counting = tests::alloc_counter::Counting;
