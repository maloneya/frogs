//! Soft-step contact solver, after Erin Catto's Solver2D and Box2D v3.
//!
//! A tick collides once per collision pass, then runs a fixed number of
//! substeps. Each substep reapplies the previous impulses (warm start), solves
//! once with soft overlap correction, integrates positions, and relaxes once
//! without correction so push-out leaves no velocity behind. Work per contact
//! is fixed: nothing iterates to a tolerance. Steady loads such as a crowd
//! leaning on a latch converge across substeps and ticks through warm starting.
//!
//! Contacts are speculative. A pair whose gap could close during a pass is
//! constrained before it touches, so fast bodies stop at the surface instead
//! of tunnelling. A pass shortens when anything moves fast enough to exceed
//! that reach; bodies still beyond it are swept against solids afterwards.
use crate::contact_memory::{Contact, Key, Memory};
use crate::panel::{lever, spin_velocity};
use crate::pass::motion::Physics;
use crate::{ARENA_HALF, Dt, ENEMY_RADIUS, EntityId, Event, LatchState, PLAYER_RADIUS, TraceSink, body_radius};
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
    Panel(u32),
    Solid,
}

/// Any contact involving a hinged panel. Anchors are relative to the body
/// centre or the hinge at collision time.
#[derive(Clone, Copy)]
struct Hinged {
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

/// Per-pass panel working state; hinges are fixed, so only the angle moves.
#[derive(Clone, Copy, Default)]
struct Hinge {
    omega: f32,
    inverse_inertia: f32,
    angle: f32,
    turned: f32,
    lower: f32,
    upper: f32,
    /// Angular impulse the latch has absorbed this tick, and its strength.
    spent: f32,
    strength: f32,
    latched: bool,
    min: f32,
    max: f32,
    /// Distance from hinge to the farthest corner.
    corner: f32,
}

/// Reused solve buffers and the hashed warm-start memory.
#[derive(Default)]
pub(crate) struct Solver {
    states: Vec<State>,
    panel_of: Vec<u32>,
    start: Vec<Vec2>,
    pairs: Vec<Pair>,
    pair_keys: Vec<Key>,
    solids: Vec<Solid>,
    solid_keys: Vec<Key>,
    hinged: Vec<Hinged>,
    hinged_keys: Vec<Key>,
    hinges: Vec<Hinge>,
    panel_rows: Vec<usize>,
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
        reserve_capacity(&mut self.panel_of, bodies);
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
    // Panel yaw is clockwise on the ground plane; see `PanelState::centre`.
    let (sin, cos) = angle.sin_cos();
    Vec2::new(cos * r.x + sin * r.y, -sin * r.x + cos * r.y)
}
fn canonical(ids: &[EntityId], a: usize, b: usize, normal: Vec2) -> Vec2 {
    if ids[a] > ids[b] { -normal } else { normal }
}

/// Resolve contacts, stops and latches for one tick. Motor changes and
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
    observer.physics_count(Counter::LatchBreaks, latches(&mut s, physics, &mut trace));
    s.touched.clear();
    s.touched.resize(ids.len(), 0);

    let tick = dt.secs();
    observer.physics_phase(Phase::Broadphase);
    let panel_of = &s.panel_of;
    grid.build(positions, |row| panel_of[row] == NONE);
    let hot = mark(&mut s, positions, physics, grid, tick);
    // Only a body or panel corner that could outrun speculative reach during
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
    for hinge in &mut s.hinges {
        hinge.lower *= h;
        hinge.upper *= h;
    }

    let mut work = crate::CrowdWork::default();
    let mut player_contacts = 0;
    let mut transfers = 0;
    for pass in 0..passes {
        let speed = top_speed(&s);
        let margin = BASE_MARGIN + 2.0 * (speed * pass_time).min(MAX_TRAVEL);
        s.start.clear();
        s.start.extend_from_slice(positions);
        for hinge in &mut s.hinges {
            hinge.turned = 0.0;
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
            let panel_of = &s.panel_of;
            grid.build(positions, |row| panel_of[row] == NONE);
            observer.physics_phase(Phase::CrowdContacts);
            active_pairs(&mut s, ids, positions, grid, margin);
            observer.physics_phase(Phase::SolidContacts);
            active_solids(&mut s, &geometry, positions, margin);
            fresh
        };
        collide_panels(&mut s, &geometry, positions, physics, grid, margin);
        observer.physics_count(Counter::BodyRows, (s.pairs.len() - fresh.0) as u64);
        observer.physics_count(
            Counter::SolidRows,
            (s.solids.len() - fresh.1 + s.hinged.len()) as u64,
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
                + s.hinged.iter().filter(|c| c.separation > 0.0).count();
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
            observer.physics_count(Counter::LatchBreaks, latches(&mut s, physics, &mut trace));
        }
        // Later passes and the continuous sweep collide against the current pose.
        for (hinge, panel) in s.hinges.iter().zip(&mut physics.panels.rows) {
            panel.omega = hinge.omega;
            panel.angle = hinge.angle;
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

/// Dense per-row body state and per-panel hinge state for this tick.
fn load(s: &mut Solver, ids: &[EntityId], physics: &mut Physics) {
    s.states.clear();
    s.panel_of.clear();
    for &id in ids {
        let motion = physics.get(id).expect("physical body");
        let panel = physics.panels.index(id);
        s.states.push(State {
            velocity: motion.velocity(),
            drive: physics.take_drive(id),
            moved: Vec2::ZERO,
            inverse_mass: if panel.is_some() {
                0.0
            } else {
                motion.inverse_mass()
            },
        });
        s.panel_of.push(panel.map_or(NONE, |i| i as u32));
    }
    s.panel_rows.clear();
    s.panel_rows.resize(physics.panels.rows.len(), 0);
    for (row, &panel) in s.panel_of.iter().enumerate() {
        if panel != NONE {
            s.panel_rows[panel as usize] = row;
        }
    }
    s.hinges.clear();
    for p in &mut physics.panels.rows {
        p.previous = p.angle;
        p.reaction = 0.0;
        let latched = p.latch == LatchState::Latched;
        s.hinges.push(Hinge {
            omega: p.omega,
            inverse_inertia: p.inverse_inertia,
            angle: p.angle,
            // Stop torques become impulses once the substep length is known.
            lower: if latched { 0.0 } else { p.lower },
            upper: if latched { 0.0 } else { p.upper },
            strength: p.config.latch_impulse() * p.config.width(),
            latched,
            min: p.config.min_angle(),
            max: p.config.max_angle(),
            corner: p.config.reach(),
            ..Hinge::default()
        });
    }
    // A latched panel is immovable: any spin a direct impulse gave it this
    // tick is load its latch must absorb.
    for hinge in &mut s.hinges {
        if hinge.latched {
            hinge.spent = hinge.omega / hinge.inverse_inertia;
            hinge.omega = 0.0;
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

/// Fastest body or panel corner; bounds how far anything can move this pass.
fn top_speed(s: &Solver) -> f32 {
    let bodies = s
        .states
        .iter()
        .filter(|state| state.inverse_mass > 0.0)
        .map(|state| (state.velocity + state.drive).length_squared())
        .fold(0.0, f32::max)
        .sqrt();
    s.hinges
        .iter()
        .map(|hinge| hinge.omega.abs() * hinge.corner)
        .fold(bodies, f32::max)
}

/// Bodies and panel corners fast enough to outrun speculative reach during the
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
        if s.panel_of[row] != NONE
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
    for (hinge, p) in s.hinges.iter().zip(&physics.panels.rows) {
        let speed = hinge.omega.abs() * hinge.corner;
        if speed * tick <= MAX_TRAVEL {
            continue;
        }
        hot = hot.max(speed);
        let swing = Vec2::splat(hinge.corner) + reach;
        let active = &mut s.active;
        grid.query(p.hinge - swing, p.hinge + swing, |other| {
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
        if s.panel_of[row] != NONE || s.states[row].inverse_mass == 0.0 {
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
    for i in 0..s.hinged.len() {
        let (c, key) = (s.hinged[i], s.hinged_keys[i]);
        transfers += tally_hinged(s, c, key);
        let normal = hinged_normal(c, key, ids, physics);
        s.carried.record_any(Contact {
            key,
            normal,
            force: c.impulse * inv_h,
        });
    }
    s.carried.accept();
    transfers
}

/// A panel row's normal in the memory's canonical orientation.
fn hinged_normal(c: Hinged, key: Key, ids: &[EntityId], physics: &Physics) -> Vec2 {
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
        if s.active[row] && s.panel_of[row] == NONE {
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
    for c in &mut s.hinged {
        c.total += c.impulse;
    }
}

/// Write solved velocities back. Four substep shares of a motor's change need
/// not sum exactly in f32; when nothing else acted, keep the motor's exact
/// result so a braking body can come exactly to rest.
fn finish(s: &Solver, ids: &[EntityId], physics: &mut Physics) {
    for (row, &id) in ids.iter().enumerate() {
        if s.panel_of[row] == NONE {
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
    for (hinge, panel) in s.hinges.iter().zip(&mut physics.panels.rows) {
        panel.reaction = hinge.spent.abs() / panel.config.width();
    }
}

/// Panel rows are rebuilt every pass: panels are few and rotate.
fn collide_panels(
    s: &mut Solver,
    geometry: &Geometry<'_>,
    positions: &[Vec2],
    physics: &Physics,
    grid: &crate::grid::Grid,
    margin: f32,
) {
    let ids = geometry.ids;
    s.hinged.clear();
    s.hinged_keys.clear();
    for (i, p) in physics.panels.rows.iter().enumerate() {
        let shape = p.shape();
        let body = |s: &mut Solver, row: usize| {
            let r = body_radius(row);
            let (normal, separation) = shape.circle_gap(positions[row], r);
            if separation <= margin {
                let point = positions[row] - normal * (r + 0.5 * separation);
                s.hinged.push(Hinged {
                    a: End::Panel(i as u32),
                    b: End::Body(row as u32),
                    normal,
                    ra: point - p.hinge,
                    rb: Vec2::ZERO,
                    separation: separation - s.states[row].moved.dot(normal),
                    mass: 0.0,
                    impulse: 0.0,
                    total: 0.0,
                });
                s.hinged_keys.push(Key::bodies(p.id, ids[row]));
            }
        };
        let widen = Vec2::splat(PLAYER_RADIUS + margin);
        let mut rows = core::mem::take(&mut s.movers_scratch);
        rows.clear();
        grid.query(
            shape.centre() - shape.bounds() - widen,
            shape.centre() + shape.bounds() + widen,
            |row| rows.push(row as u32),
        );
        for &row in &rows {
            body(s, row as usize);
        }
        s.movers_scratch = rows;
        for (owner, index, wall) in geometry.walls.iter() {
            let gap = (wall.centre() - shape.centre()).abs() - wall.bounds() - shape.bounds();
            if gap.max_element() > margin {
                continue;
            }
            if let Some((normal, points, depth)) = wall.rectangle_contact(shape, margin) {
                for (k, point) in points.into_iter().enumerate() {
                    let Some(point) = point else { continue };
                    s.hinged.push(Hinged {
                        a: End::Solid,
                        b: End::Panel(i as u32),
                        normal,
                        ra: Vec2::ZERO,
                        rb: point - p.hinge,
                        separation: -depth,
                        mass: 0.0,
                        impulse: 0.0,
                        total: 0.0,
                    });
                    s.hinged_keys.push(Key::Wall(p.id, owner, index, k as u8));
                }
            }
        }
        for (j, q) in physics.panels.rows[..i].iter().enumerate() {
            if let Some((normal, points, depth)) = q.shape().rectangle_contact(shape, margin) {
                for (k, point) in points.into_iter().enumerate() {
                    let Some(point) = point else { continue };
                    s.hinged.push(Hinged {
                        a: End::Panel(j as u32),
                        b: End::Panel(i as u32),
                        normal,
                        ra: point - q.hinge,
                        rb: point - p.hinge,
                        separation: -depth,
                        mass: 0.0,
                        impulse: 0.0,
                        total: 0.0,
                    });
                    s.hinged_keys.push(Key::bodies(q.id, p.id).point(k as u8));
                }
            }
        }
    }
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
    hinged_masses(s);
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
    for (c, &key) in s.hinged.iter_mut().zip(&s.hinged_keys) {
        c.impulse = if c.mass == 0.0 {
            0.0
        } else {
            force(key, hinged_normal(*c, key, ids, physics)) * h
        };
    }
}

/// A latched panel takes no motion from contacts; its latch absorbs the load.
fn hinged_masses(s: &mut Solver) {
    for c in &mut s.hinged {
        let k = |end: End, r: Vec2| match end {
            End::Body(row) => s.states[row as usize].inverse_mass,
            End::Panel(i) => {
                let hinge = &s.hinges[i as usize];
                if hinge.latched {
                    0.0
                } else {
                    hinge.inverse_inertia * lever(r, c.normal).powi(2)
                }
            }
            End::Solid => 0.0,
        };
        let inverse = k(c.a, c.ra) + k(c.b, c.rb);
        c.mass = if inverse > 0.0 { 1.0 / inverse } else { 0.0 };
    }
}

fn immovable(states: &[State], hinges: &[Hinge], end: End) -> bool {
    match end {
        End::Body(row) => states[row as usize].inverse_mass == 0.0,
        End::Panel(i) => hinges[i as usize].latched,
        End::Solid => true,
    }
}

/// A latch fails once this tick's absorbed load exceeds its strength. It
/// keeps exactly its strength; the excess becomes the panel's spin, so the
/// breach carries the momentum that caused it.
fn latches(s: &mut Solver, physics: &mut Physics, trace: &mut TraceSink<'_>) -> u64 {
    let mut broke = 0;
    for (hinge, panel) in s.hinges.iter_mut().zip(&mut physics.panels.rows) {
        if hinge.latched && hinge.spent.abs() > hinge.strength {
            let kept = hinge.spent.clamp(-hinge.strength, hinge.strength);
            hinge.omega = hinge.inverse_inertia * (hinge.spent - kept);
            hinge.spent = kept;
            hinge.latched = false;
            panel.latch = LatchState::Broken;
            trace.emit(Event::LatchBroken { id: panel.id() });
            broke += 1;
        }
    }
    if broke > 0 {
        hinged_masses(s);
    }
    broke
}
fn end_id(end: End, ids: &[EntityId], physics: &Physics) -> EntityId {
    match end {
        End::Body(row) => ids[row as usize],
        End::Panel(i) => physics.panels.rows[i as usize].id,
        End::Solid => unreachable!("solids have no identity"),
    }
}

fn apply(states: &mut [State], hinges: &mut [Hinge], end: End, r: Vec2, impulse: Vec2) {
    match end {
        End::Body(row) => {
            let state = &mut states[row as usize];
            state.velocity += impulse * state.inverse_mass;
        }
        End::Panel(i) => {
            let hinge = &mut hinges[i as usize];
            if hinge.latched {
                hinge.spent += lever(r, impulse);
            } else {
                hinge.omega += hinge.inverse_inertia * lever(r, impulse);
            }
        }
        End::Solid => {}
    }
}
fn point_velocity(states: &[State], hinges: &[Hinge], end: End, r: Vec2) -> Vec2 {
    match end {
        End::Body(row) => states[row as usize].velocity,
        End::Panel(i) => spin_velocity(r, hinges[i as usize].omega),
        End::Solid => Vec2::ZERO,
    }
}
fn displacement(states: &[State], hinges: &[Hinge], end: End, r: Vec2) -> Vec2 {
    match end {
        End::Body(row) => states[row as usize].moved,
        End::Panel(i) => rotate(r, hinges[i as usize].turned) - r,
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
    for hinge in &mut s.hinges {
        if !hinge.latched {
            hinge.omega += hinge.inverse_inertia * (hinge.lower - hinge.upper);
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
    for c in &s.hinged {
        let impulse = c.normal * c.impulse;
        apply(&mut s.states, &mut s.hinges, c.a, c.ra, -impulse);
        apply(&mut s.states, &mut s.hinges, c.b, c.rb, impulse);
    }
}

/// One Gauss-Seidel sweep: soft and biased when `bias`, rigid relaxation otherwise.
/// Angular stops come first, as in Box2D.
fn iterate(s: &mut Solver, t: Tuning, bias: bool) {
    for hinge in &mut s.hinges {
        if hinge.latched {
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
    for c in &mut s.hinged {
        if c.mass == 0.0 {
            continue;
        }
        let moved = displacement(&s.states, &s.hinges, c.b, c.rb)
            - displacement(&s.states, &s.hinges, c.a, c.ra);
        let separation = c.separation + moved.dot(c.normal);
        let soft = if immovable(&s.states, &s.hinges, c.a) || immovable(&s.states, &s.hinges, c.b) {
            t.fixed
        } else {
            t.contact
        };
        let Some((velocity_bias, mass_scale, impulse_scale)) =
            soft.at(separation, t.inv_h, bias, t.push)
        else {
            continue;
        };
        let vn = (point_velocity(&s.states, &s.hinges, c.b, c.rb)
            - point_velocity(&s.states, &s.hinges, c.a, c.ra))
        .dot(c.normal);
        let impulse = -c.mass * mass_scale * (vn + velocity_bias) - impulse_scale * c.impulse;
        let total = (c.impulse + impulse).max(0.0);
        let impulse = c.normal * (total - c.impulse);
        c.impulse = total;
        apply(&mut s.states, &mut s.hinges, c.a, c.ra, -impulse);
        apply(&mut s.states, &mut s.hinges, c.b, c.rb, impulse);
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
    for (i, (hinge, p)) in s.hinges.iter_mut().zip(&physics.panels.rows).enumerate() {
        // Like Box2D's rotation limit: a corner may not outrun one pass's
        // speculative reach. Only spins far beyond gameplay impulses reach it.
        let spin = MAX_TRAVEL / (hinge.corner * pass_time);
        hinge.omega = hinge.omega.clamp(-spin, spin);
        let before = hinge.angle;
        if hinge.latched {
            hinge.angle = 0.0;
        } else {
            hinge.angle = (hinge.angle + hinge.omega * h).clamp(hinge.min, hinge.max);
        }
        hinge.turned += hinge.angle - before;
        let mut pose = *p;
        pose.angle = hinge.angle;
        positions[s.panel_rows[i]] = pose.centre();
    }
}

/// Bodies moving beyond the speculative reach cannot tunnel through solids:
/// sweep their pass, sliding along walls and the arena edge and stopping at a
/// panel surface. Velocity is kept; the next pass's contacts resolve it.
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
        for p in &physics.panels.rows {
            if let Some(hit) = p.shape().sweep(start, end - start, r) {
                end = hit.point() + hit.normal() * crate::wall::SKIN;
            }
        }
        state.moved += end - positions[row];
        positions[row] = end;
    }
    swept
}

/// Count a panel row's reportable exchange: momentum with a body or panel, or
/// a panel held by its frame (which touches a wall like any body).
fn tally_hinged(s: &mut Solver, c: Hinged, key: Key) -> usize {
    if c.total <= REPORTED {
        return 0;
    }
    match (c.a, key) {
        (End::Solid, _) | (_, Key::Wall(..) | Key::Arena(..)) => {
            if let End::Panel(i) = c.b {
                s.touched[s.panel_rows[i as usize]] |= 1;
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
    for i in 0..s.hinged.len() {
        let (c, key) = (s.hinged[i], s.hinged_keys[i]);
        transfers += tally_hinged(s, c, key);
        let normal = hinged_normal(c, key, ids, physics);
        s.memory.record(Contact {
            key,
            normal,
            force: force(c.impulse),
        });
    }
    s.memory.accept();
    for (hinge, p) in s.hinges.iter().zip(&mut physics.panels.rows) {
        p.lower = hinge.lower * inv_h;
        p.upper = hinge.upper * inv_h;
    }
    transfers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Accumulator, Impulse, Mass, MotorCommand, PanelConfig, PanelSpec, Placed, Scene, Template,
        World,
    };
    use arpg_core::{AimDir, Intent, MoveDir};

    fn tick() -> Dt {
        Accumulator::default().pending(Dt::SECS).next().unwrap()
    }
    fn idle() -> Intent {
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false)
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
        world.step(tick(), idle());
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
        let config = PanelConfig::new(2.0, 0.2, 0.0, -1.5, 1.5, 10000.0).unwrap();
        let mut scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY.with_mass(Mass::try_from(2.0).unwrap()),
            }],
            panels: vec![PanelSpec { body: 0, config }],
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
                .begin_step(tick(), idle(), 0)
                .advance(&commands, &[], &mut ())
                .finish();
        }
        world.panel(ids[1]).unwrap().reaction_impulse()
    }

    #[test]
    fn a_driven_chain_transmits_its_combined_motor_load_in_either_body_order() {
        // Each blocked motor supplies m * a * dt = 1 kg * 60 m/s² / 60 Hz of
        // momentum per tick at the panel's midpoint: half that at its free edge.
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
    fn a_latch_spends_its_strength_and_passes_the_excess_in_the_same_tick() {
        let strength = 0.5;
        let config = PanelConfig::new(2.0, 0.1, 0.0, -1.5, 1.5, strength).unwrap();
        let scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY,
            }],
            panels: vec![PanelSpec { body: 0, config }],
            ..Scene::default()
        };
        for (impulse, breaks) in [(0.45, false), (1.5, true)] {
            let mut world = World::from_scene(&scene).unwrap();
            let panel = world.bodies.slots.ids()[1];
            assert!(world.apply_impulse_at(
                panel,
                Vec2::new(11.0, 10.0),
                Impulse::try_from((0.0, -impulse)).unwrap()
            ));
            world.step(tick(), idle());
            let state = world.panel(panel).unwrap();
            assert_eq!(state.latch() == LatchState::Broken, breaks);
            if breaks {
                assert!((state.reaction_impulse() - strength).abs() < 1e-4);
                assert!(state.angle() > 0.0, "the excess opened the panel this tick");
            } else {
                assert!((state.reaction_impulse() - impulse).abs() < 1e-4);
                assert_eq!(state.angle(), 0.0);
            }
        }
    }

    #[test]
    fn a_new_breaching_impulse_cannot_sweep_a_panel_through_a_fixed_prop() {
        let config = PanelConfig::new(2.0, 0.1, 0.0, -1.5, 1.5, 0.5).unwrap();
        let mut scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY,
            }],
            panels: vec![PanelSpec { body: 0, config }],
            ..Scene::default()
        };
        scene.bodies.push(Placed {
            pos: (10.6, 9.15),
            what: Template::BLOCK,
        });
        let mut world = World::from_scene(&scene).unwrap();
        let panel = world.bodies.slots.ids()[1];
        assert!(world.apply_impulse_at(
            panel,
            Vec2::new(11.0, 10.0),
            Impulse::try_from((0.0, -150.0)).unwrap()
        ));
        for _ in 0..3 {
            world.step(tick(), idle());
            let state = world.panel(panel).unwrap();
            assert_eq!(state.latch(), LatchState::Broken);
            assert!(
                state.angle() < 0.65,
                "panel passed fixed prop: angle {}",
                state.angle()
            );
        }
        assert!(
            world.panel(panel).unwrap().angle() > 0.1,
            "impact did not move panel"
        );
    }

    #[test]
    fn a_released_panel_refuses_motor_commands() {
        let config = PanelConfig::new(2.0, 0.1, 0.0, -1.5, 1.5, 10.0).unwrap();
        let scene = Scene {
            bodies: vec![Placed {
                pos: (10.0, 10.0),
                what: Template::BODY,
            }],
            panels: vec![PanelSpec { body: 0, config }],
            ..Scene::default()
        };
        let [mut driven, mut still] = [(); 2].map(|()| World::from_scene(&scene).unwrap());
        let panel = driven.bodies.slots.ids()[1];
        for world in [&mut driven, &mut still] {
            assert!(world.bodies.physics.panels.release(panel));
        }
        let command = MotorCommand::new(panel, Vec2::new(0.0, 3.0), 30.0).unwrap();
        for _ in 0..30 {
            let prepared = driven.begin_step(tick(), idle(), 0);
            assert!(!prepared.view().movable(panel));
            prepared.advance(&[command], &[], &mut ()).finish();
            still.begin_step(tick(), idle(), 0).advance(&[], &[], &mut ()).finish();
            assert_eq!(driven.hash(), still.hash());
        }
        assert_eq!(driven.panel(panel).unwrap().angle(), 0.0);
    }
}
