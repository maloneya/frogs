//! Soft-step contact solver, after Erin Catto's Solver2D and Box2D v3.
//!
//! A tick collides once per collision pass, then runs a fixed number of
//! substeps. Each substep reapplies the previous impulses (warm start), solves
//! once with soft overlap correction, integrates positions, and relaxes once
//! without correction so push-out leaves no velocity behind. Work per contact
//! is fixed: nothing iterates to a tolerance. Steady loads such as a crowd
//! leaning on a lock converge across substeps and ticks through warm starting.
//!
//! Contacts are speculative. A pair whose gap could close during a pass is
//! constrained before it touches, so fast bodies stop at the surface instead
//! of tunnelling. A pass shortens when anything moves fast enough to exceed
//! that reach; bodies still beyond it are swept against solids afterwards.
//!
//! Jointed bodies collide through their current box. A hinge turns in this
//! plane; a topple joint tips out of it (see `pass::topple`) and is immovable
//! here. A held lock is immovable too: it absorbs what contacts deliver and
//! fails once a tick's load exceeds its strength.
use crate::contact_memory::{Contact, Key, Memory};
use crate::joint::{JointConfig, Kind, lever};
use crate::pass::motion::Physics;
use crate::{ARENA_HALF, Dt, ENEMY_RADIUS, EntityId, Event, LockState, PLAYER_RADIUS, TraceSink, body_radius};
use crate::{PhysicsCounter as Counter, PhysicsPhase as Phase, StepObserver};
use glam::Vec2;

/// Resting pairs within this gap are already constrained (Box2D's speculative distance).
const BASE_MARGIN: f32 = 0.02;
/// Largest distance any body may travel during one collision pass.
const MAX_TRAVEL: f32 = 0.16;
/// Two bodies approaching each other at the pass's top speed, plus the resting margin.
const MAX_MARGIN: f32 = BASE_MARGIN + 2.0 * MAX_TRAVEL;
/// Largest centre distance from a solid surface at which a contact can form.
pub(crate) const MAX_REACH: f32 = PLAYER_RADIUS + MAX_MARGIN;
const _: () = assert!(PLAYER_RADIUS + ENEMY_RADIUS + MAX_MARGIN <= crate::grid::CELL);
const SUBSTEPS: usize = 4;
/// Collision passes available to fast motion before the continuous sweep takes over.
const MAX_PASSES: usize = 8;
/// Box2D caps contact stiffness at an eighth of the substep rate for stability.
pub(crate) const MAX_CONTACT_HERTZ: f32 = 0.125 * (SUBSTEPS as u32 * crate::time::TICK_HZ) as f32;
/// Box2D's damping ratio for joint limits.
const JOINT_DAMPING: f32 = 2.0;
/// Overlap within this slop is held rigidly instead of pushed apart, so a
/// settled crowd comes to rest rather than creeping toward exact contact.
const SLOP: f32 = 0.0005;
/// Slowest push-out. A pure spring approaches the slop band exponentially and
/// never quite arrives; this floor lands inside it in finite time. One substep
/// at this speed moves less than the band is wide.
const MIN_PUSH: f32 = 0.05;
const _: () = assert!(MIN_PUSH * Dt::SECS / SUBSTEPS as f32 <= SLOP);
/// Overlap deeper than this counts as crowding in traces and telemetry.
const OVERLAP: f32 = 0.001;
/// Impulses below this are not remembered for warm starting.
const IMPULSE: f32 = 1e-6;
/// Impulses below this (0.1 mm/s on a kilogram) are float chatter at exact
/// contact, not reportable momentum exchange.
const REPORTED: f32 = 1e-4;
const COINCIDENT_SQ: f32 = 1e-12;
const NONE: u32 = u32::MAX;

/// Implicit spring-damper coefficients for one substep length.
#[derive(Clone, Copy)]
struct Soft {
    bias: f32,
    mass: f32,
    impulse: f32,
}
impl Soft {
    fn new(hertz: f32, damping: f32, h: f32) -> Self {
        let omega = core::f32::consts::TAU * hertz;
        let a1 = 2.0 * damping + h * omega;
        let a2 = h * omega * a1;
        let a3 = 1.0 / (1.0 + a2);
        Self {
            bias: omega / a1,
            mass: a2 * a3,
            impulse: a3,
        }
    }
    /// Speculative approach, soft correction, or rigid relaxation. Relaxation
    /// leaves open gaps alone: the next substep's solve re-checks them before
    /// anything moves, with that substep's own motor share applied.
    fn at(self, separation: f32, inv_h: f32, bias: bool, push: f32) -> Option<(f32, f32, f32)> {
        if separation > 0.0 {
            bias.then_some((separation * inv_h, 1.0, 0.0))
        } else if bias && separation < -SLOP {
            let bias = (self.bias * (separation + SLOP)).clamp(-push, -MIN_PUSH.min(push));
            Some((bias, self.mass, self.impulse))
        } else {
            Some((0.0, 1.0, 0.0))
        }
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    velocity: Vec2,
    /// Motor velocity change applied evenly across the tick's substeps.
    drive: Vec2,
    /// Displacement since this pass's collision snapshot.
    moved: Vec2,
    inverse_mass: f32,
}
impl State {
    /// Fastest speed during the tick before any contact: velocity ramps
    /// linearly from its start to its motor target, so an endpoint bounds it.
    fn bound(self) -> f32 {
        self.velocity
            .length()
            .max((self.velocity + self.drive).length())
    }
}

/// Two circular bodies; either may be fixed.
#[derive(Clone, Copy)]
struct Pair {
    a: u32,
    b: u32,
    normal: Vec2,
    separation: f32,
    mass: f32,
    impulse: f32,
    /// Impulse delivered across the whole pass, for reporting momentum exchange.
    total: f32,
    fixed: bool,
}

/// A circular body against a wall or the arena edge.
#[derive(Clone, Copy)]
struct Solid {
    body: u32,
    normal: Vec2,
    separation: f32,
    mass: f32,
    impulse: f32,
    total: f32,
    arena: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum End {
    Body(u32),
    Joint(u32),
    Solid,
}

/// Any contact involving a jointed body. Anchors are relative to the body
/// centre or the joint's pivot at collision time.
#[derive(Clone, Copy)]
struct Jointed {
    a: End,
    b: End,
    normal: Vec2,
    ra: Vec2,
    rb: Vec2,
    separation: f32,
    mass: f32,
    impulse: f32,
    total: f32,
}

/// Per-tick joint working state. Pivots are fixed, so only the coordinate
/// moves, and only a hinge moves in this plane.
#[derive(Clone, Copy)]
struct Work {
    config: JointConfig,
    omega: f32,
    inverse_inertia: f32,
    angle: f32,
    turned: f32,
    lower: f32,
    upper: f32,
    min: f32,
    max: f32,
    /// Distance from the pivot to the farthest corner that turns in this plane.
    corner: f32,
    /// Load at the arm this tick: absorbed by a held lock or, once a pole's
    /// lock has gone, the tipping load its base passes on.
    spent: Vec2,
    strength: f32,
    held: bool,
    /// Whether contacts and impulses add to `spent`.
    absorbs: bool,
    broke: bool,
}
impl Work {
    fn inverse_mass(&self, r: Vec2, normal: Vec2) -> f32 {
        if self.immovable() {
            0.0
        } else {
            self.inverse_inertia * lever(r, normal).powi(2)
        }
    }
    fn planar(&self) -> bool {
        self.config.planar()
    }
    fn immovable(&self) -> bool {
        self.held || !self.planar()
    }
    fn apply(&mut self, r: Vec2, impulse: Vec2) {
        if !self.immovable() {
            self.omega += self.inverse_inertia * lever(r, impulse);
        } else if self.absorbs {
            self.spent += self.config.at_arm(r, impulse);
        }
    }
    fn velocity(&self, r: Vec2) -> Vec2 {
        self.config.velocity_at(r, self.omega)
    }
    fn displacement(&self, r: Vec2) -> Vec2 {
        if self.planar() {
            rotate(r, self.turned) - r
        } else {
            Vec2::ZERO
        }
    }
    /// A held lock fails once this tick's load exceeds its strength. It keeps
    /// exactly its strength; the excess becomes a hinge's spin, or a pole's
    /// tipping load, in the direction of the load.
    fn fail(&mut self) -> Option<Vec2> {
        if !self.held || self.spent.length() <= self.strength {
            return None;
        }
        let direction = self.spent.normalize();
        self.spent -= direction * self.strength;
        self.held = false;
        self.broke = true;
        if self.planar() {
            self.omega = self.config.rate_from(self.spent.x, self.inverse_inertia);
            self.spent = Vec2::ZERO;
            self.absorbs = false;
        }
        Some(direction)
    }
}

/// Reused solve buffers and the hashed warm-start memory.
#[derive(Default)]
pub(crate) struct Solver {
    states: Vec<State>,
    joint_of: Vec<u32>,
    start: Vec<Vec2>,
    pairs: Vec<Pair>,
    pair_keys: Vec<Key>,
    solids: Vec<Solid>,
    solid_keys: Vec<Key>,
    jointed: Vec<Jointed>,
    jointed_keys: Vec<Key>,
    joints: Vec<Work>,
    joint_rows: Vec<usize>,
    /// Rows re-collided every pass; see `mark`.
    active: Vec<bool>,
    /// Each body's speed bound for the tick, computed once.
    bounds: Vec<f32>,
    movers: Vec<u32>,
    movers_scratch: Vec<u32>,
    touched: Vec<u8>,
    memory: Memory,
    /// Forces of active rows retired between passes, so the next pass's fresh
    /// rows continue an impact instead of restarting it. Scratch, not state.
    carried: Memory,
}
impl Solver {
    pub(crate) fn revoke(&mut self, id: EntityId) {
        self.memory.revoke(id);
    }
    pub(crate) fn evict(&mut self, owner: crate::SceneId) {
        self.memory.evict(owner);
    }
    pub(crate) fn clear_cache(&mut self) {
        self.memory.clear();
    }
    pub(crate) fn hash(&self, h: &mut crate::Fnv) {
        self.memory.hash(h);
    }
    pub(crate) fn report(&self, out: &mut arpg_core::Report) {
        self.memory.report(out);
    }
    pub(crate) fn reserve(&mut self, bodies: usize) {
        use crate::reserve_capacity;
        // Ordinary local contact graphs fit this population-scaled reservation.
        // Denser admitted geometry may grow it; this is never a contact limit.
        let contacts = bodies * 4;
        reserve_capacity(&mut self.states, bodies);
        reserve_capacity(&mut self.joint_of, bodies);
        reserve_capacity(&mut self.start, bodies);
        reserve_capacity(&mut self.touched, bodies);
        reserve_capacity(&mut self.active, bodies);
        reserve_capacity(&mut self.bounds, bodies);
        reserve_capacity(&mut self.movers, bodies);
        reserve_capacity(&mut self.movers_scratch, bodies);
        reserve_capacity(&mut self.pairs, contacts);
        reserve_capacity(&mut self.pair_keys, contacts);
        reserve_capacity(&mut self.solids, bodies);
        reserve_capacity(&mut self.solid_keys, bodies);
        self.memory.reserve(bodies, contacts);
        self.carried.reserve(bodies, contacts);
    }
}

/// Static inputs for a tick's contact solve.
pub(crate) struct Geometry<'a> {
    pub(crate) ids: &'a [EntityId],
    pub(crate) walls: &'a crate::wall::Walls,
    pub(crate) material: crate::tuning::ContactMaterial,
}

/// Stable golden-angle escape direction so coincident pairs spread instead of restacking.
fn escape(tiebreak: usize) -> Vec2 {
    let angle = tiebreak as f32 * crate::angle::GOLDEN_ANGLE;
    Vec2::new(angle.cos(), angle.sin())
}
fn rotate(r: Vec2, angle: f32) -> Vec2 {
    // Hinge yaw is clockwise on the ground plane; see `JointState::collider`.
    let (sin, cos) = angle.sin_cos();
    Vec2::new(cos * r.x + sin * r.y, -sin * r.x + cos * r.y)
}
fn canonical(ids: &[EntityId], a: usize, b: usize, normal: Vec2) -> Vec2 {
    if ids[a] > ids[b] { -normal } else { normal }
}

/// Resolve contacts, stops and locks for one tick. Motor changes and
/// impulses arrive from the caller once per tick; motors ramp in per substep.
///
/// This generic function only marks observer phases and calls non-generic
/// workers. Generic code is compiled in the calling crate, so keeping the
/// loops out of it keeps them compiled, and optimized, with the simulation.
pub(crate) fn solve<O: StepObserver>(
    geometry: Geometry<'_>,
    positions: &mut [Vec2],
    physics: &mut Physics,
    grid: &mut crate::grid::Grid,
    dt: Dt,
    mut trace: TraceSink<'_>,
    observer: &mut O,
) -> (crate::CrowdWork, usize) {
    let mut s = core::mem::take(&mut physics.solver);
    let ids = geometry.ids;
    load(&mut s, ids, physics);
    observer.physics_count(Counter::LockBreaks, locks(&mut s, physics, &mut trace));
    s.touched.clear();
    s.touched.resize(ids.len(), 0);

    let tick = dt.secs();
    observer.physics_phase(Phase::Broadphase);
    let joint_of = &s.joint_of;
    grid.build(positions, |row| joint_of[row] == NONE);
    let hot = mark(&mut s, positions, physics, grid, tick);
    // Only a body or hinge corner that could outrun speculative reach during
    // the tick splits it; slow ticks take a single collision pass.
    let passes = if hot * tick > MAX_TRAVEL {
        ((hot * tick / MAX_TRAVEL).ceil() as usize).clamp(1, MAX_PASSES)
    } else {
        1
    };
    let substeps = SUBSTEPS.div_ceil(passes);
    let pass_time = tick / passes as f32;
    let h = pass_time / substeps as f32;
    let material = geometry.material;
    let hertz = material.frequency_hz().min(0.125 / h);
    let tuning = Tuning {
        inv_h: 1.0 / h,
        push: material.push_speed(),
        contact: Soft::new(hertz, material.damping_ratio(), h),
        fixed: Soft::new(2.0 * hertz, material.damping_ratio(), h),
        joint: Soft::new(2.0 * hertz, JOINT_DAMPING, h),
    };
    for joint in &mut s.joints {
        joint.lower *= h;
        joint.upper *= h;
    }

    let mut work = crate::CrowdWork::default();
    let mut player_contacts = 0;
    let mut transfers = 0;
    for pass in 0..passes {
        let speed = top_speed(&s);
        let margin = BASE_MARGIN + 2.0 * (speed * pass_time).min(MAX_TRAVEL);
        s.start.clear();
        s.start.extend_from_slice(positions);
        for joint in &mut s.joints {
            joint.turned = 0.0;
        }
        observer.physics_count(Counter::CollisionPasses, 1);
        let fresh = if pass == 0 {
            let reach = Reach { margin, tick };
            observer.physics_phase(Phase::CrowdContacts);
            let candidates = crowd_pairs(
                &mut s,
                ids,
                positions,
                grid,
                reach,
                (&mut work, &mut player_contacts),
            );
            observer.physics_count(Counter::Candidates, candidates);
            observer.physics_phase(Phase::SolidContacts);
            quiet_solids(&mut s, &geometry, positions, reach);
            (0, 0)
        } else {
            transfers += retire(&mut s, ids, physics, h);
            let fresh = (s.pairs.len(), s.solids.len());
            observer.physics_phase(Phase::Broadphase);
            let joint_of = &s.joint_of;
            grid.build(positions, |row| joint_of[row] == NONE);
            observer.physics_phase(Phase::CrowdContacts);
            active_pairs(&mut s, ids, positions, grid, margin);
            observer.physics_phase(Phase::SolidContacts);
            active_solids(&mut s, &geometry, positions, margin);
            fresh
        };
        collide_joints(&mut s, &geometry, positions, physics, grid, margin);
        observer.physics_count(Counter::BodyRows, (s.pairs.len() - fresh.0) as u64);
        observer.physics_count(
            Counter::SolidRows,
            (s.solids.len() - fresh.1 + s.jointed.len()) as u64,
        );
        if O::PHYSICS_DETAILS {
            let speculative = s.pairs[fresh.0..]
                .iter()
                .filter(|c| c.separation > 0.0)
                .count()
                + s.solids[fresh.1..]
                    .iter()
                    .filter(|c| c.separation > 0.0)
                    .count()
                + s.jointed.iter().filter(|c| c.separation > 0.0).count();
            observer.physics_count(Counter::SpeculativeRows, speculative as u64);
        }
        observer.physics_phase(Phase::Prepare);
        prepare(&mut s, ids, physics, h, fresh, pass > 0);
        for _ in 0..substeps {
            observer.physics_count(Counter::Substeps, 1);
            observer.physics_phase(Phase::WarmStart);
            accelerate(&mut s, (passes * substeps) as f32);
            warm_start(&mut s);
            observer.physics_phase(Phase::Solve);
            iterate(&mut s, tuning, true);
            observer.physics_phase(Phase::Integrate);
            integrate(&mut s, positions, physics, h, pass_time);
            observer.physics_phase(Phase::Relax);
            iterate(&mut s, tuning, false);
            accumulate(&mut s);
            observer.physics_count(Counter::LockBreaks, locks(&mut s, physics, &mut trace));
        }
        // Later passes and the continuous sweep collide against the current pose.
        for (work, joint) in s.joints.iter().zip(&mut physics.joints.rows) {
            if work.planar() {
                joint.rate = work.omega;
                joint.angle = work.angle;
            }
        }
        if speed * pass_time > MAX_TRAVEL {
            observer.physics_phase(Phase::Continuous);
            observer.physics_count(
                Counter::ContinuousBodies,
                sweep(&mut s, &geometry, positions, physics),
            );
        }
    }
    observer.physics_phase(Phase::Store);
    transfers += store(&mut s, ids, physics, h);
    observer.physics_phase(Phase::Other);
    finish(&s, ids, physics);
    if work.contacts > 0 {
        trace.emit(Event::Crowded {
            count: work.contacts,
        });
    }
    if player_contacts > 0 {
        trace.emit(Event::Contacts {
            count: player_contacts,
        });
    }
    if transfers > 0 {
        trace.emit(Event::Momentum { pairs: transfers });
    }
    let walls = s.touched.iter().filter(|&&mask| mask & 1 != 0).count();
    let arena = s.touched.iter().filter(|&&mask| mask & 2 != 0).count();
    if walls > 0 {
        trace.emit(Event::WallContacts { count: walls });
    }
    if arena > 0 {
        trace.emit(Event::Clamped { count: arena });
    }
    physics.solver = s;
    (work, player_contacts)
}

/// Dense per-row body state and per-joint working state for this tick.
fn load(s: &mut Solver, ids: &[EntityId], physics: &mut Physics) {
    s.states.clear();
    s.joint_of.clear();
    for &id in ids {
        let motion = physics.get(id).expect("physical body");
        let joint = physics.joints.index(id);
        s.states.push(State {
            velocity: motion.velocity(),
            drive: physics.take_drive(id),
            moved: Vec2::ZERO,
            inverse_mass: if joint.is_some() {
                0.0
            } else {
                motion.inverse_mass()
            },
        });
        s.joint_of.push(joint.map_or(NONE, |i| i as u32));
    }
    s.joint_rows.clear();
    s.joint_rows.resize(physics.joints.rows.len(), 0);
    for (row, &joint) in s.joint_of.iter().enumerate() {
        if joint != NONE {
            s.joint_rows[joint as usize] = row;
        }
    }
    s.joints.clear();
    for j in &mut physics.joints.rows {
        j.previous = j.angle;
        let held = j.lock == LockState::Held;
        let ((min, max), (lower, upper)) = match j.kind {
            Kind::Hinge { config, lower, upper } => ((config.min_angle(), config.max_angle()), (lower, upper)),
            Kind::Topple { .. } => ((0.0, 0.0), (0.0, 0.0)),
        };
        let planar = j.config().planar();
        let absorbs = held || j.base_turns();
        // Direct impulses since the last solve are this tick's first load.
        // A held joint is immovable, and a pivot that no longer turns under
        // base contact discards them.
        s.joints.push(Work {
            config: j.config(),
            omega: if held || !planar { 0.0 } else { j.rate },
            inverse_inertia: j.inverse_inertia,
            angle: j.angle,
            turned: 0.0,
            // Stop torques become impulses once the substep length is known.
            lower: if held { 0.0 } else { lower },
            upper: if held { 0.0 } else { upper },
            min,
            max,
            corner: if planar { j.point_speed(1.0) } else { 0.0 },
            spent: if absorbs { j.pending } else { Vec2::ZERO },
            strength: j.strength,
            held,
            absorbs,
            broke: false,
        });
        j.pending = Vec2::ZERO;
        j.reaction = 0.0;
        if let Kind::Topple { load, .. } = &mut j.kind {
            *load = Vec2::ZERO;
        }
    }
}

#[derive(Clone, Copy)]
struct Tuning {
    inv_h: f32,
    push: f32,
    contact: Soft,
    fixed: Soft,
    joint: Soft,
}

/// Fastest body or hinge corner; bounds how far anything can move this pass.
fn top_speed(s: &Solver) -> f32 {
    let bodies = s
        .states
        .iter()
        .filter(|state| state.inverse_mass > 0.0)
        .map(|state| (state.velocity + state.drive).length_squared())
        .fold(0.0, f32::max)
        .sqrt();
    s.joints
        .iter()
        .map(|joint| joint.omega.abs() * joint.corner)
        .fold(bodies, f32::max)
}

/// Bodies and hinge corners fast enough to outrun speculative reach during the
/// tick are hot. They, and everything within two cells of their path, form the
/// active region that is re-collided every collision pass: anything there can
/// be struck to speed mid-tick. Everything else is collided once, with margins
/// sized to its own speed for the whole tick. Returns the fastest hot speed.
fn mark(
    s: &mut Solver,
    positions: &[Vec2],
    physics: &Physics,
    grid: &crate::grid::Grid,
    tick: f32,
) -> f32 {
    s.active.clear();
    s.active.resize(positions.len(), false);
    s.bounds.clear();
    s.bounds.extend(s.states.iter().map(|state| state.bound()));
    let reach = Vec2::splat(2.0 * crate::grid::CELL);
    let mut hot = 0.0_f32;
    for (row, &position) in positions.iter().enumerate() {
        let state = s.states[row];
        if s.joint_of[row] != NONE
            || state.inverse_mass == 0.0
            || s.bounds[row] * tick <= MAX_TRAVEL
        {
            continue;
        }
        hot = hot.max(s.bounds[row]);
        let end = position + (state.velocity + state.drive) * tick;
        let active = &mut s.active;
        grid.query(
            position.min(end) - reach,
            position.max(end) + reach,
            |other| {
                active[other] = true;
            },
        );
        s.active[row] = true;
    }
    for (joint, p) in s.joints.iter().zip(&physics.joints.rows) {
        let speed = joint.omega.abs() * joint.corner;
        if speed * tick <= MAX_TRAVEL {
            continue;
        }
        hot = hot.max(speed);
        let swing = Vec2::splat(joint.corner) + reach;
        let active = &mut s.active;
        grid.query(p.anchor - swing, p.anchor + swing, |other| {
            active[other] = true
        });
    }
    hot
}

/// Speculative reach for the first collision pass.
#[derive(Clone, Copy)]
struct Reach {
    /// Margin for rows in the active region, covering one pass at top speed.
    margin: f32,
    /// Tick length: quiet rows persist for the whole tick.
    tick: f32,
}

#[inline]
fn pair(
    s: &Solver,
    positions: &[Vec2],
    ids: &[EntityId],
    (a, b): (usize, usize),
    margin: impl FnOnce(&Solver) -> f32,
) -> Option<(Pair, Key)> {
    // Most grid candidates are out of any reach: reject them before reading
    // body state.
    let touching = body_radius(a) + body_radius(b);
    let delta = positions[b] - positions[a];
    let distance_sq = delta.length_squared();
    let widest = touching + MAX_MARGIN;
    if distance_sq > widest * widest {
        return None;
    }
    let reach = touching + margin(s);
    if distance_sq > reach * reach {
        return None;
    }
    let (sa, sb) = (s.states[a], s.states[b]);
    if sa.inverse_mass == 0.0 && sb.inverse_mass == 0.0 {
        return None;
    }
    let distance = distance_sq.sqrt();
    let normal = if distance_sq > COINCIDENT_SQ {
        delta / distance
    } else {
        escape(a + b)
    };
    // Separation is measured against displacement since the tick began, so a
    // row made in a later pass reads the same way as one kept from the first.
    let separation = distance - touching - (sb.moved - sa.moved).dot(normal);
    Some((
        Pair {
            a: a as u32,
            b: b as u32,
            normal,
            separation,
            mass: 0.0,
            impulse: 0.0,
            total: 0.0,
            fixed: sa.inverse_mass == 0.0 || sb.inverse_mass == 0.0,
        },
        Key::bodies(ids[a], ids[b]),
    ))
}

fn solids(
    s: &mut Solver,
    walls: &crate::wall::Walls,
    ids: &[EntityId],
    row: usize,
    position: Vec2,
    margin: f32,
) {
    let r = body_radius(row);
    let moved = s.states[row].moved;
    for (owner, index, wall) in walls.near(position) {
        let (normal, separation) = wall.circle_contact(position, r);
        if separation <= margin {
            s.solids.push(Solid {
                body: row as u32,
                normal,
                separation: separation - moved.dot(normal),
                mass: 0.0,
                impulse: 0.0,
                total: 0.0,
                arena: false,
            });
            s.solid_keys.push(Key::Wall(ids[row], owner, index, 0));
        }
    }
    for axis in 0..2 {
        for (side, sign) in [-1.0_f32, 1.0].into_iter().enumerate() {
            let separation = ARENA_HALF - r - position[axis] * sign;
            if separation <= margin {
                let mut normal = Vec2::ZERO;
                normal[axis] = -sign;
                s.solids.push(Solid {
                    body: row as u32,
                    normal,
                    separation: separation - moved.dot(normal),
                    mass: 0.0,
                    impulse: 0.0,
                    total: 0.0,
                    arena: true,
                });
                s.solid_keys
                    .push(Key::Arena(ids[row], (axis * 2 + side) as u8));
            }
        }
    }
}

/// First pass: every circle pair. Quiet pairs get margins sized to their own
/// speeds for the whole tick and persist; active pairs cover one pass.
fn crowd_pairs(
    s: &mut Solver,
    ids: &[EntityId],
    positions: &[Vec2],
    grid: &crate::grid::Grid,
    reach: Reach,
    (work, player): (&mut crate::CrowdWork, &mut usize),
) -> u64 {
    s.pairs.clear();
    s.pair_keys.clear();
    let margin_of = |s: &Solver, a: usize, b: usize| {
        if s.active[a] || s.active[b] {
            reach.margin
        } else {
            BASE_MARGIN + (s.bounds[a] + s.bounds[b]) * reach.tick
        }
    };
    let mut found = core::mem::take(&mut s.pairs);
    let mut keys = core::mem::take(&mut s.pair_keys);
    let candidates = grid.pairs(|i, j| {
        let (a, b) = (i.min(j), i.max(j));
        if let Some((c, key)) = pair(s, positions, ids, (a, b), |s| margin_of(s, a, b)) {
            if c.separation < -OVERLAP {
                if a == 0 {
                    *player += 1;
                } else {
                    work.contacts += 1;
                }
            }
            found.push(c);
            keys.push(key);
        }
    });
    s.pairs = found;
    s.pair_keys = keys;
    let n = positions.len() as u64;
    work.candidates += candidates;
    work.possible_pairs += n.saturating_sub(1) * n.saturating_sub(2) / 2;
    candidates
}

/// First pass: walls and the arena edge for every movable body.
fn quiet_solids(s: &mut Solver, geometry: &Geometry<'_>, positions: &[Vec2], reach: Reach) {
    let ids = geometry.ids;
    s.solids.clear();
    s.solid_keys.clear();
    for (row, &position) in positions.iter().enumerate() {
        if s.joint_of[row] != NONE || s.states[row].inverse_mass == 0.0 {
            continue;
        }
        let margin = if s.active[row] {
            reach.margin
        } else {
            BASE_MARGIN + s.bounds[row] * reach.tick
        };
        solids(s, geometry.walls, ids, row, position, margin);
    }
}

/// Retire rows touching the active region; the pass re-collides them. Their
/// momentum exchange so far still counts, and their forces carry over.
fn retire(s: &mut Solver, ids: &[EntityId], physics: &Physics, h: f32) -> usize {
    let mut transfers = 0;
    let active = &s.active;
    let inv_h = 1.0 / h;
    s.carried.begin();
    let mut kept = 0;
    for i in 0..s.pairs.len() {
        let c = s.pairs[i];
        if active[c.a as usize] || active[c.b as usize] {
            transfers += usize::from(c.total > REPORTED);
            s.carried.record_any(Contact {
                key: s.pair_keys[i],
                normal: canonical(ids, c.a as usize, c.b as usize, c.normal),
                force: c.impulse * inv_h,
            });
        } else {
            s.pairs[kept] = c;
            s.pair_keys[kept] = s.pair_keys[i];
            kept += 1;
        }
    }
    s.pairs.truncate(kept);
    s.pair_keys.truncate(kept);
    let mut kept = 0;
    for i in 0..s.solids.len() {
        let c = s.solids[i];
        if active[c.body as usize] {
            if c.total > REPORTED {
                s.touched[c.body as usize] |= if c.arena { 2 } else { 1 };
            }
            s.carried.record_any(Contact {
                key: s.solid_keys[i],
                normal: c.normal,
                force: c.impulse * inv_h,
            });
        } else {
            s.solids[kept] = c;
            s.solid_keys[kept] = s.solid_keys[i];
            kept += 1;
        }
    }
    s.solids.truncate(kept);
    s.solid_keys.truncate(kept);
    for i in 0..s.jointed.len() {
        let (c, key) = (s.jointed[i], s.jointed_keys[i]);
        transfers += tally_jointed(s, c, key);
        let normal = jointed_normal(c, key, ids, physics);
        s.carried.record_any(Contact {
            key,
            normal,
            force: c.impulse * inv_h,
        });
    }
    s.carried.accept();
    transfers
}

/// A jointed row's normal in the memory's canonical orientation.
fn jointed_normal(c: Jointed, key: Key, ids: &[EntityId], physics: &Physics) -> Vec2 {
    match (c.a, key) {
        (End::Solid, _) | (_, Key::Wall(..) | Key::Arena(..)) => c.normal,
        (_, Key::Bodies(low, ..)) => {
            if end_id(c.a, ids, physics) == low {
                c.normal
            } else {
                -c.normal
            }
        }
    }
}

/// Later passes: fresh rows for active bodies only, found through the grid
/// rebuilt at this pass's positions. Each pair is made once: from its active
/// body when the other is quiet, from the lower row when both are active.
fn active_pairs(
    s: &mut Solver,
    ids: &[EntityId],
    positions: &[Vec2],
    grid: &crate::grid::Grid,
    margin: f32,
) {
    s.movers.clear();
    for row in 0..positions.len() {
        if s.active[row] && s.joint_of[row] == NONE {
            s.movers.push(row as u32);
        }
    }
    let widen = Vec2::splat(PLAYER_RADIUS + ENEMY_RADIUS + margin);
    let mut found = core::mem::take(&mut s.pairs);
    let mut keys = core::mem::take(&mut s.pair_keys);
    for m in 0..s.movers.len() {
        let a = s.movers[m] as usize;
        grid.query(positions[a] - widen, positions[a] + widen, |b| {
            if b == a || (s.active[b] && b < a) {
                return;
            }
            if let Some((c, key)) = pair(s, positions, ids, (a.min(b), a.max(b)), |_| margin) {
                found.push(c);
                keys.push(key);
            }
        });
    }
    s.pairs = found;
    s.pair_keys = keys;
}

fn active_solids(s: &mut Solver, geometry: &Geometry<'_>, positions: &[Vec2], margin: f32) {
    for m in 0..s.movers.len() {
        let row = s.movers[m] as usize;
        if s.states[row].inverse_mass > 0.0 {
            solids(s, geometry.walls, geometry.ids, row, positions[row], margin);
        }
    }
}

/// Accumulate each row's impulse over the pass, for reporting.
fn accumulate(s: &mut Solver) {
    for c in &mut s.pairs {
        c.total += c.impulse;
    }
    for c in &mut s.solids {
        c.total += c.impulse;
    }
    for c in &mut s.jointed {
        c.total += c.impulse;
    }
}

/// Write solved velocities back. Four substep shares of a motor's change need
/// not sum exactly in f32; when nothing else acted, keep the motor's exact
/// result so a braking body can come exactly to rest.
fn finish(s: &Solver, ids: &[EntityId], physics: &mut Physics) {
    for (row, &id) in ids.iter().enumerate() {
        if s.joint_of[row] == NONE {
            let state = s.states[row];
            let nominal = physics.get(id).expect("physical body").velocity() + state.drive;
            let velocity =
                if (state.velocity - nominal).length() <= 1e-6 * nominal.length().max(1.0) {
                    nominal
                } else {
                    state.velocity
                };
            physics.set_velocity(id, velocity);
        }
    }
    for (work, joint) in s.joints.iter().zip(&mut physics.joints.rows) {
        joint.reaction = if work.held {
            work.spent.length()
        } else if work.broke {
            work.strength
        } else {
            0.0
        };
        if !work.held
            && let Kind::Topple { load, .. } = &mut joint.kind
        {
            *load = work.spent;
        }
    }
}

/// Joint rows are rebuilt every pass: joints are few, and their boxes turn or
/// are replaced. Joints that cannot move in this plane skip static solids.
fn collide_joints(
    s: &mut Solver,
    geometry: &Geometry<'_>,
    positions: &[Vec2],
    physics: &Physics,
    grid: &crate::grid::Grid,
    margin: f32,
) {
    let ids = geometry.ids;
    s.jointed.clear();
    s.jointed_keys.clear();
    let mut rows = core::mem::take(&mut s.movers_scratch);
    for (i, p) in physics.joints.rows.iter().enumerate() {
        let shape = p.collider();
        let planar = s.joints[i].planar();
        let widen = Vec2::splat(PLAYER_RADIUS + margin);
        rows.clear();
        grid.query(
            shape.centre() - shape.bounds() - widen,
            shape.centre() + shape.bounds() + widen,
            |row| rows.push(row as u32),
        );
        for &row in &rows {
            let row = row as usize;
            let r = body_radius(row);
            let (normal, separation) = shape.circle_gap(positions[row], r);
            if separation <= margin {
                let point = positions[row] - normal * (r + 0.5 * separation);
                s.jointed.push(Jointed {
                    a: End::Joint(i as u32),
                    b: End::Body(row as u32),
                    normal,
                    ra: point - p.anchor,
                    rb: Vec2::ZERO,
                    separation: separation - s.states[row].moved.dot(normal),
                    mass: 0.0,
                    impulse: 0.0,
                    total: 0.0,
                });
                s.jointed_keys.push(Key::bodies(p.id, ids[row]));
            }
        }
        if planar {
            for (owner, index, wall) in geometry.walls.iter() {
                if !wall.bounds_within(shape, margin) {
                    continue;
                }
                if let Some((normal, points, depth)) = wall.rectangle_contact(shape, margin) {
                    for (k, point) in points.into_iter().enumerate() {
                        let Some(point) = point else { continue };
                        s.jointed.push(Jointed {
                            a: End::Solid,
                            b: End::Joint(i as u32),
                            normal,
                            ra: Vec2::ZERO,
                            rb: point - p.anchor,
                            separation: -depth,
                            mass: 0.0,
                            impulse: 0.0,
                            total: 0.0,
                        });
                        s.jointed_keys.push(Key::Wall(p.id, owner, index, k as u8));
                    }
                }
            }
        }
        for (j, q) in physics.joints.rows[..i].iter().enumerate() {
            if !planar && !s.joints[j].planar() {
                continue;
            }
            if let Some((normal, points, depth)) = q.collider().rectangle_contact(shape, margin) {
                for (k, point) in points.into_iter().enumerate() {
                    let Some(point) = point else { continue };
                    s.jointed.push(Jointed {
                        a: End::Joint(j as u32),
                        b: End::Joint(i as u32),
                        normal,
                        ra: point - q.anchor,
                        rb: point - p.anchor,
                        separation: -depth,
                        mass: 0.0,
                        impulse: 0.0,
                        total: 0.0,
                    });
                    s.jointed_keys.push(Key::bodies(q.id, p.id).point(k as u8));
                }
            }
        }
    }
    s.movers_scratch = rows;
}

/// Effective masses and warm-start impulses for rows new this pass. Rows kept
/// from an earlier pass keep their current impulse as the warm start.
fn prepare(
    s: &mut Solver,
    ids: &[EntityId],
    physics: &Physics,
    h: f32,
    fresh: (usize, usize),
    later: bool,
) {
    jointed_masses(s);
    // A later pass continues its own previous pass; otherwise last tick.
    let (memory, carried) = (&s.memory, &s.carried);
    let force = |key: Key, normal: Vec2| {
        later
            .then(|| carried.find(key, normal))
            .flatten()
            .unwrap_or_else(|| memory.force(key, normal))
    };
    for (c, &key) in s.pairs[fresh.0..].iter_mut().zip(&s.pair_keys[fresh.0..]) {
        let (a, b) = (c.a as usize, c.b as usize);
        c.mass = 1.0 / (s.states[a].inverse_mass + s.states[b].inverse_mass);
        c.impulse = force(key, canonical(ids, a, b, c.normal)) * h;
    }
    for (c, &key) in s.solids[fresh.1..].iter_mut().zip(&s.solid_keys[fresh.1..]) {
        c.mass = 1.0 / s.states[c.body as usize].inverse_mass;
        c.impulse = force(key, c.normal) * h;
    }
    for (c, &key) in s.jointed.iter_mut().zip(&s.jointed_keys) {
        c.impulse = if c.mass == 0.0 {
            0.0
        } else {
            force(key, jointed_normal(*c, key, ids, physics)) * h
        };
    }
}

/// A held lock or an out-of-plane joint takes no motion from contacts.
fn jointed_masses(s: &mut Solver) {
    for c in &mut s.jointed {
        let k = |end: End, r: Vec2| match end {
            End::Body(row) => s.states[row as usize].inverse_mass,
            End::Joint(i) => s.joints[i as usize].inverse_mass(r, c.normal),
            End::Solid => 0.0,
        };
        let inverse = k(c.a, c.ra) + k(c.b, c.rb);
        c.mass = if inverse > 0.0 { 1.0 / inverse } else { 0.0 };
    }
}

fn immovable(states: &[State], joints: &[Work], end: End) -> bool {
    match end {
        End::Body(row) => states[row as usize].inverse_mass == 0.0,
        End::Joint(i) => joints[i as usize].immovable(),
        End::Solid => true,
    }
}

/// Fail every lock whose load this tick exceeds its strength; see `Work::fail`.
fn locks(s: &mut Solver, physics: &mut Physics, trace: &mut TraceSink<'_>) -> u64 {
    let mut broke = 0;
    for (work, joint) in s.joints.iter_mut().zip(&mut physics.joints.rows) {
        if let Some(direction) = work.fail() {
            joint.lock = LockState::Broken;
            if let Kind::Topple { direction: fall, .. } = &mut joint.kind {
                *fall = direction;
            }
            trace.emit(Event::LockBroken { id: joint.id() });
            broke += 1;
        }
    }
    if broke > 0 {
        jointed_masses(s);
    }
    broke
}
fn end_id(end: End, ids: &[EntityId], physics: &Physics) -> EntityId {
    match end {
        End::Body(row) => ids[row as usize],
        End::Joint(i) => physics.joints.rows[i as usize].id,
        End::Solid => unreachable!("solids have no identity"),
    }
}

fn apply(states: &mut [State], joints: &mut [Work], end: End, r: Vec2, impulse: Vec2) {
    match end {
        End::Body(row) => {
            let state = &mut states[row as usize];
            state.velocity += impulse * state.inverse_mass;
        }
        End::Joint(i) => joints[i as usize].apply(r, impulse),
        End::Solid => {}
    }
}
fn point_velocity(states: &[State], joints: &[Work], end: End, r: Vec2) -> Vec2 {
    match end {
        End::Body(row) => states[row as usize].velocity,
        End::Joint(i) => joints[i as usize].velocity(r),
        End::Solid => Vec2::ZERO,
    }
}
fn displacement(states: &[State], joints: &[Work], end: End, r: Vec2) -> Vec2 {
    match end {
        End::Body(row) => states[row as usize].moved,
        End::Joint(i) => joints[i as usize].displacement(r),
        End::Solid => Vec2::ZERO,
    }
}

/// Motors act like forces: an equal share of the tick's drive each substep.
fn accelerate(s: &mut Solver, substeps: f32) {
    for state in &mut s.states {
        state.velocity += state.drive / substeps;
    }
}

fn warm_start(s: &mut Solver) {
    for joint in &mut s.joints {
        if !joint.immovable() {
            joint.omega += joint.inverse_inertia * (joint.lower - joint.upper);
        }
    }
    for c in &s.pairs {
        let impulse = c.normal * c.impulse;
        let a = &mut s.states[c.a as usize];
        a.velocity -= impulse * a.inverse_mass;
        let b = &mut s.states[c.b as usize];
        b.velocity += impulse * b.inverse_mass;
    }
    for c in &s.solids {
        let b = &mut s.states[c.body as usize];
        b.velocity += c.normal * (c.impulse * b.inverse_mass);
    }
    for c in &s.jointed {
        let impulse = c.normal * c.impulse;
        apply(&mut s.states, &mut s.joints, c.a, c.ra, -impulse);
        apply(&mut s.states, &mut s.joints, c.b, c.rb, impulse);
    }
}

/// One Gauss-Seidel sweep: soft and biased when `bias`, rigid relaxation otherwise.
/// Angular stops come first, as in Box2D.
fn iterate(s: &mut Solver, t: Tuning, bias: bool) {
    for hinge in &mut s.joints {
        if hinge.immovable() {
            continue;
        }
        let mass = 1.0 / hinge.inverse_inertia;
        if let Some((b, m, i)) = t
            .joint
            .at(hinge.angle - hinge.min, t.inv_h, bias, f32::INFINITY)
        {
            let impulse = -mass * m * (hinge.omega + b) - i * hinge.lower;
            let lower = (hinge.lower + impulse).max(0.0);
            hinge.omega += hinge.inverse_inertia * (lower - hinge.lower);
            hinge.lower = lower;
        }
        if let Some((b, m, i)) = t
            .joint
            .at(hinge.max - hinge.angle, t.inv_h, bias, f32::INFINITY)
        {
            let impulse = -mass * m * (-hinge.omega + b) - i * hinge.upper;
            let upper = (hinge.upper + impulse).max(0.0);
            hinge.omega -= hinge.inverse_inertia * (upper - hinge.upper);
            hinge.upper = upper;
        }
    }
    for c in &mut s.pairs {
        let (a, b) = (c.a as usize, c.b as usize);
        let (sa, sb) = (s.states[a], s.states[b]);
        let separation = c.separation + (sb.moved - sa.moved).dot(c.normal);
        let soft = if c.fixed { t.fixed } else { t.contact };
        let Some((velocity_bias, mass_scale, impulse_scale)) =
            soft.at(separation, t.inv_h, bias, t.push)
        else {
            continue;
        };
        let vn = (sb.velocity - sa.velocity).dot(c.normal);
        let impulse = -c.mass * mass_scale * (vn + velocity_bias) - impulse_scale * c.impulse;
        let total = (c.impulse + impulse).max(0.0);
        let impulse = c.normal * (total - c.impulse);
        c.impulse = total;
        s.states[a].velocity = sa.velocity - impulse * sa.inverse_mass;
        s.states[b].velocity = sb.velocity + impulse * sb.inverse_mass;
    }
    for c in &mut s.solids {
        let state = &mut s.states[c.body as usize];
        let separation = c.separation + state.moved.dot(c.normal);
        let Some((velocity_bias, mass_scale, impulse_scale)) =
            t.fixed.at(separation, t.inv_h, bias, t.push)
        else {
            continue;
        };
        let vn = state.velocity.dot(c.normal);
        let impulse = -c.mass * mass_scale * (vn + velocity_bias) - impulse_scale * c.impulse;
        let total = (c.impulse + impulse).max(0.0);
        state.velocity += c.normal * ((total - c.impulse) * state.inverse_mass);
        c.impulse = total;
    }
    for c in &mut s.jointed {
        if c.mass == 0.0 {
            continue;
        }
        let moved = displacement(&s.states, &s.joints, c.b, c.rb)
            - displacement(&s.states, &s.joints, c.a, c.ra);
        let separation = c.separation + moved.dot(c.normal);
        let soft = if immovable(&s.states, &s.joints, c.a) || immovable(&s.states, &s.joints, c.b) {
            t.fixed
        } else {
            t.contact
        };
        let Some((velocity_bias, mass_scale, impulse_scale)) =
            soft.at(separation, t.inv_h, bias, t.push)
        else {
            continue;
        };
        let vn = (point_velocity(&s.states, &s.joints, c.b, c.rb)
            - point_velocity(&s.states, &s.joints, c.a, c.ra))
        .dot(c.normal);
        let impulse = -c.mass * mass_scale * (vn + velocity_bias) - impulse_scale * c.impulse;
        let total = (c.impulse + impulse).max(0.0);
        let impulse = c.normal * (total - c.impulse);
        c.impulse = total;
        apply(&mut s.states, &mut s.joints, c.a, c.ra, -impulse);
        apply(&mut s.states, &mut s.joints, c.b, c.rb, impulse);
    }
}

fn integrate(s: &mut Solver, positions: &mut [Vec2], physics: &Physics, h: f32, pass_time: f32) {
    for (row, state) in s.states.iter_mut().enumerate() {
        if state.inverse_mass == 0.0 {
            continue;
        }
        let limit = ARENA_HALF - body_radius(row);
        // Authored bodies may start outside the arena; containment is absolute.
        // Clamping would hide a NaN inside the arena, so catch it first.
        let next = positions[row] + state.velocity * h;
        debug_assert!(next.is_finite(), "non-finite solver output for row {row}");
        let next = next.clamp(Vec2::splat(-limit), Vec2::splat(limit));
        state.moved += next - positions[row];
        positions[row] = next;
    }
    for (i, (hinge, p)) in s.joints.iter_mut().zip(&physics.joints.rows).enumerate() {
        if !hinge.planar() {
            continue;
        }
        // Like Box2D's rotation limit: a corner may not outrun one pass's
        // speculative reach. Only spins far beyond gameplay impulses reach it.
        let spin = MAX_TRAVEL / (hinge.corner * pass_time);
        hinge.omega = hinge.omega.clamp(-spin, spin);
        let before = hinge.angle;
        if !hinge.held {
            hinge.angle = (hinge.angle + hinge.omega * h).clamp(hinge.min, hinge.max);
        }
        hinge.turned += hinge.angle - before;
        let mut pose = *p;
        pose.angle = hinge.angle;
        positions[s.joint_rows[i]] = pose.collider().centre();
    }
}

/// Bodies moving beyond the speculative reach cannot tunnel through solids:
/// sweep their pass, sliding along walls and the arena edge and stopping at a
/// jointed surface. Velocity is kept; the next pass's contacts resolve it.
fn sweep(
    s: &mut Solver,
    geometry: &Geometry<'_>,
    positions: &mut [Vec2],
    physics: &Physics,
) -> u64 {
    let mut swept = 0;
    for (row, (state, &start)) in s.states.iter_mut().zip(&s.start).enumerate() {
        let travel = positions[row] - start;
        if state.inverse_mass == 0.0 || travel.length_squared() <= MAX_TRAVEL * MAX_TRAVEL {
            continue;
        }
        swept += 1;
        let r = body_radius(row);
        let mut end = geometry.walls.slide(start, positions[row], r);
        for joint in &physics.joints.rows {
            if let Some(hit) = joint.collider().sweep(start, end - start, r) {
                end = hit.point() + hit.normal() * crate::wall::SKIN;
            }
        }
        state.moved += end - positions[row];
        positions[row] = end;
    }
    swept
}

/// Count a jointed row's reportable exchange: momentum with a body or joint,
/// or a hinge held by its frame (which touches a wall like any body).
fn tally_jointed(s: &mut Solver, c: Jointed, key: Key) -> usize {
    if c.total <= REPORTED {
        return 0;
    }
    match (c.a, key) {
        (End::Solid, _) | (_, Key::Wall(..) | Key::Arena(..)) => {
            if let End::Joint(i) = c.b {
                s.touched[s.joint_rows[i as usize]] |= 1;
            }
            0
        }
        (_, Key::Bodies(..)) => 1,
    }
}

/// Keep the tick's final forces for the next warm start; count momentum
/// exchange. Negligible impulses are forgotten so resting contacts start from
/// exact zero instead of trading roundoff forever.
fn store(s: &mut Solver, ids: &[EntityId], physics: &mut Physics, h: f32) -> usize {
    let inv_h = 1.0 / h;
    let force = |impulse: f32| {
        if impulse > IMPULSE {
            impulse * inv_h
        } else {
            0.0
        }
    };
    let mut transfers = 0;
    s.memory.begin();
    for (c, &key) in s.pairs.iter().zip(&s.pair_keys) {
        let (a, b) = (c.a as usize, c.b as usize);
        transfers += usize::from(c.total > REPORTED);
        s.memory.record(Contact {
            key,
            normal: canonical(ids, a, b, c.normal),
            force: force(c.impulse),
        });
    }
    for (c, &key) in s.solids.iter().zip(&s.solid_keys) {
        if c.total > REPORTED {
            s.touched[c.body as usize] |= if c.arena { 2 } else { 1 };
        }
        s.memory.record(Contact {
            key,
            normal: c.normal,
            force: force(c.impulse),
        });
    }
    for i in 0..s.jointed.len() {
        let (c, key) = (s.jointed[i], s.jointed_keys[i]);
        transfers += tally_jointed(s, c, key);
        let normal = jointed_normal(c, key, ids, physics);
        s.memory.record(Contact {
            key,
            normal,
            force: force(c.impulse),
        });
    }
    s.memory.accept();
    for (work, joint) in s.joints.iter().zip(&mut physics.joints.rows) {
        if let Kind::Hinge { lower, upper, .. } = &mut joint.kind {
            *lower = work.lower * inv_h;
            *upper = work.upper * inv_h;
        }
    }
    transfers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Accumulator, HingeConfig, Impulse, JointConfig, JointSpec, LockState, LockStrength, Mass,
        MotorCommand, Placed, Scene, Template, ToppleConfig,
        World,
    };

    fn tick() -> Dt {
        Accumulator::default().pending(Dt::SECS).next().unwrap()
    }
    /// A 2 m hinge bound to the scene's first body, held by `strength`.
    fn hinge(thickness: f32, strength: f32) -> Vec<JointSpec> {
        vec![JointSpec {
            body: 0,
            joint: JointConfig::Hinge(HingeConfig::new(2.0, thickness, 0.0, -1.5, 1.5).unwrap()),
            lock: LockStrength::try_from(strength).unwrap(),
        }]
    }

    #[test]
    fn soft_coefficients_match_the_implicit_spring_damper() {
        let h = 1.0 / 240.0;
        let soft = Soft::new(30.0, 10.0, h);
        let omega = core::f32::consts::TAU * 30.0;
        let a1 = 20.0 + h * omega;
        assert!((soft.bias - omega / a1).abs() < 1e-4);
        assert!((soft.mass + soft.impulse - 1.0).abs() < 1e-6);
        let (speculative, mass, impulse) = soft.at(0.01, 240.0, true, 3.0).unwrap();
        assert!((speculative - 2.4).abs() < 1e-5 && mass == 1.0 && impulse == 0.0);
        assert_eq!(
            soft.at(-1.0, 240.0, true, 3.0).unwrap().0,
            -3.0,
            "push speed caps correction"
        );
        assert_eq!(
            soft.at(-SLOP * 0.5, 240.0, true, 3.0),
            Some((0.0, 1.0, 0.0)),
            "slop rests"
        );
        assert_eq!(
            soft.at(0.01, 240.0, false, 3.0),
            None,
            "relaxation leaves open gaps"
        );
        assert_eq!(soft.at(-1.0, 240.0, false, 3.0), Some((0.0, 1.0, 0.0)));
    }

    #[test]
    fn a_head_on_collision_conserves_momentum_and_cannot_add_energy() {
        let scene = Scene {
            bodies: vec![
                Placed {
                    pos: (10.0, 10.0),
                    what: Template::BODY.with_mass(Mass::try_from(2.0).unwrap()),
                },
                Placed {
                    pos: (10.6, 10.0),
                    what: Template::BODY,
                },
            ],
            ..Scene::default()
        };
        let mut world = World::from_scene(&scene).unwrap();
        let ids = world.bodies.slots.ids().to_vec();
        assert!(world.apply_impulse(ids[1], Impulse::try_from((6.0, 0.0)).unwrap()));
        assert!(world.apply_impulse(ids[2], Impulse::try_from((-3.0, 0.0)).unwrap()));
        let totals = |world: &World| {
            let (a, b) = (world.motion(ids[1]).unwrap(), world.motion(ids[2]).unwrap());
            (
                2.0 * a.velocity() + b.velocity(),
                a.velocity().length_squared() + 0.5 * b.velocity().length_squared(),
            )
        };
        let (momentum, energy) = totals(&world);
        world.begin_step(tick(), 0).advance(&[], &mut ()).finish();
        let (after, after_energy) = totals(&world);
        // Passive drag scales every undriven body by the same factor after contacts.
        assert!(
            (after - momentum * 0.9).length() < 1e-4,
            "{after} vs {momentum}"
        );
        assert!(after_energy <= energy * 0.81 + 1e-4);
        let gap = world.body_pos(ids[2]).unwrap().x - world.body_pos(ids[1]).unwrap().x;
        assert!(
            gap >= 0.5 - 0.005,
            "speculative contact stops at the surface: {gap}"
        );
    }

    fn driven_chain(count: usize, reverse: bool, ticks: usize) -> f32 {
        let mut scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY.with_mass(Mass::try_from(2.0).unwrap()),
            }],
            joints: hinge(0.2, 10000.0),
            ..Scene::default()
        };
        for i in 0..count {
            let depth = if reverse { count - 1 - i } else { i };
            scene.bodies.push(Placed {
                pos: (10.0, 9.65 - depth as f32 * 0.5),
                what: Template::BODY,
            });
        }
        let mut world = World::from_scene(&scene).unwrap();
        let ids = world.bodies.slots.ids().to_vec();
        let commands: Vec<_> = ids[2..]
            .iter()
            .map(|&id| MotorCommand::new(id, Vec2::Y, 60.0).unwrap())
            .collect();
        for _ in 0..ticks {
            world
                .begin_step(tick(), 0)
                .advance(&commands, &mut ())
                .finish();
        }
        world.joint(ids[1]).unwrap().reaction_impulse()
    }

    #[test]
    fn a_driven_chain_transmits_its_combined_motor_load_in_either_body_order() {
        // Each blocked motor supplies m * a * dt = 1 kg * 60 m/s² / 60 Hz of
        // momentum per tick at the hinge's midpoint: half that at its free edge.
        for count in [1, 2, 4, 8, 16] {
            for reverse in [false, true] {
                let actual = driven_chain(count, reverse, 240);
                let expected = count as f32 * 0.5;
                assert!(
                    (actual - expected).abs() < expected * 0.001,
                    "chain {count}, reverse {reverse}: reaction {actual}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn a_lock_spends_its_strength_and_passes_the_excess_in_the_same_tick() {
        let strength = 0.5;
        let scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY,
            }],
            joints: hinge(0.1, strength),
            ..Scene::default()
        };
        for (impulse, breaks) in [(0.45, false), (1.5, true)] {
            let mut world = World::from_scene(&scene).unwrap();
            let hinged = world.bodies.slots.ids()[1];
            assert!(world.apply_impulse_at(
                hinged,
                Vec2::new(11.0, 10.0),
                Impulse::try_from((0.0, -impulse)).unwrap()
            ));
            world.begin_step(tick(), 0).advance(&[], &mut ()).finish();
            let state = world.joint(hinged).unwrap();
            assert_eq!(state.lock() == LockState::Broken, breaks);
            if breaks {
                assert!((state.reaction_impulse() - strength).abs() < 1e-4);
                assert!(state.angle() > 0.0, "the excess opened the hinge this tick");
            } else {
                assert!((state.reaction_impulse() - impulse).abs() < 1e-4);
                assert_eq!(state.angle(), 0.0);
            }
        }
    }

    #[test]
    fn a_new_breaching_impulse_cannot_sweep_a_hinge_through_a_fixed_prop() {
        let mut scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY,
            }],
            joints: hinge(0.1, 0.5),
            ..Scene::default()
        };
        scene.bodies.push(Placed {
            pos: (10.6, 9.15),
            what: Template::BLOCK,
        });
        let mut world = World::from_scene(&scene).unwrap();
        let hinged = world.bodies.slots.ids()[1];
        assert!(world.apply_impulse_at(
            hinged,
            Vec2::new(11.0, 10.0),
            Impulse::try_from((0.0, -150.0)).unwrap()
        ));
        for _ in 0..3 {
            world.begin_step(tick(), 0).advance(&[], &mut ()).finish();
            let state = world.joint(hinged).unwrap();
            assert_eq!(state.lock(), LockState::Broken);
            assert!(
                state.angle() < 0.65,
                "hinge passed fixed prop: angle {}",
                state.angle()
            );
        }
        assert!(
            world.joint(hinged).unwrap().angle() > 0.1,
            "impact did not move the hinge"
        );
    }

    fn pole_chain(count: usize, strength: f32) -> (crate::JointState, World) {
        let config = ToppleConfig::new(0.4, 4.0, 0.0, 1.0, 1.0).unwrap();
        let mut scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY.with_mass(Mass::try_from(300.0).unwrap()),
            }],
            joints: vec![JointSpec {
                body: 0,
                joint: JointConfig::Topple(config),
                lock: LockStrength::try_from(strength).unwrap(),
            }],
            ..Scene::default()
        };
        for i in 0..count {
            scene.bodies.push(Placed {
                pos: (10.0, 9.55 - i as f32 * 0.5),
                what: Template::BODY,
            });
        }
        let mut world = World::from_scene(&scene).unwrap();
        let ids = world.bodies.slots.ids().to_vec();
        let pole = ids[1];
        let commands: Vec<_> = ids[2..]
            .iter()
            .map(|&id| MotorCommand::new(id, Vec2::Y, 60.0).unwrap())
            .collect();
        for _ in 0..240 {
            world
                .begin_step(tick(), 0)
                .advance(&commands, &mut ())
                .finish();
            if world.joint(pole).unwrap().lock() != LockState::Held {
                break;
            }
        }
        (world.joint(pole).unwrap(), world)
    }

    #[test]
    fn a_pole_lock_carries_a_driven_chain_and_fails_only_when_the_load_exceeds_it() {
        // Each blocked 1 kg motor supplies 60 m/s² / 60 Hz = 1 kg m/s per tick.
        for count in [1, 3, 6] {
            let (pole, _) = pole_chain(count, 1000.0);
            assert_eq!(pole.lock(), LockState::Held);
            let expected = count as f32;
            assert!(
                (pole.reaction_impulse() - expected).abs() < expected * 0.001,
                "chain {count}: reaction {}",
                pole.reaction_impulse()
            );
        }
        // No rule counts bodies. The chain's first arrival is an impact about a
        // tenth above its steady load, so a 7.5 lock holds six pushers, and
        // yields to nine whose steady load alone exceeds it.
        assert_eq!(pole_chain(6, 7.5).0.lock(), LockState::Held);
        let (pole, world) = pole_chain(9, 7.5);
        assert_eq!(pole.lock(), LockState::Broken);
        assert_eq!(pole.reaction_impulse(), 7.5);
        assert!(pole.direction().abs_diff_eq(Vec2::Y, 1e-3));
        assert!(
            world
                .trace()
                .iter()
                .any(|(_, e)| e == Event::LockBroken { id: pole.id() })
        );
    }

    #[test]
    fn a_released_hinge_refuses_motor_commands() {
        let scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY,
            }],
            joints: hinge(0.1, 10.0),
            ..Scene::default()
        };
        let [mut driven, mut still] = [(); 2].map(|()| World::from_scene(&scene).unwrap());
        let hinged = driven.bodies.slots.ids()[1];
        for world in [&mut driven, &mut still] {
            assert!(world.bodies.physics.joints.release(hinged));
        }
        let command = MotorCommand::new(hinged, Vec2::new(0.0, 3.0), 30.0).unwrap();
        for _ in 0..30 {
            let prepared = driven.begin_step(tick(), 0);
            assert!(!prepared.view().movable(hinged));
            prepared.advance(&[command], &mut ()).finish();
            still.begin_step(tick(), 0).advance(&[], &mut ()).finish();
            assert_eq!(driven.hash(), still.hash());
        }
        assert_eq!(driven.joint(hinged).unwrap().angle(), 0.0);
    }
}
