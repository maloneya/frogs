//! What exists, and how it describes itself to a renderer.
//!
//! Depends on `arpg-core` for vocabulary and on nothing else. In particular it
//! does not link wgpu, so simulation tests run without a GPU.

use glam::{Vec2, Vec3, Vec3Swizzles};

use arpg_core::{Instance, InstanceSink, MoveDir, MAX_INSTANCES};

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
/// Sized so the world is comfortably larger than the view. A tracking camera
/// is meaningless otherwise: if the whole arena fits on screen there is nothing
/// for the camera to reveal, and following just slides the floor around inside
/// a frame that already showed everything.
///
/// The size is also what keeps the void off screen, and it is why the camera
/// does *not* clamp itself to the world bounds. Under this projection the view
/// covers roughly 57x55 world units of floor, whose axis-aligned footprint is
/// ~40 units either side of the focus. Subtract that from a 48-unit arena and a
/// bounds-clamped camera could travel +/-8 units total — it would be pinned,
/// and following would stop working before the player reached the edge. Making
/// the world bigger is the fix that a camera clamp only pretends to be.
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
/// This constant lives next to the field it bounds rather than in the caller.
/// That placement is the whole point: previously the subtraction happened in
/// `app.rs`, which meant `World` did not know its own limit and any *second*
/// writer of `enemy_count` would silently overrun the GPU buffer — a failure
/// with no error message, since the upload just truncates.
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

/// How readily a body is displaced by a contact. Inverse mass, so zero is
/// immovable and larger is lighter.
///
/// The asymmetry is the mechanic. At 1:20 the player absorbs about 5% of any
/// single separation, so one enemy is a nudge — but nothing here caps how many
/// contacts a tick may contain, and a crowd that cannot compress transmits all
/// of them. Being penned in is therefore *emergent*: no code says "blocked",
/// and the wall of bodies is only a wall because each body is also stopped by
/// the one behind it.
///
/// Inverse mass also absorbs the cases that would otherwise need their own
/// concept. A corpse, a barrel or a wall segment is a body with zero here; a
/// dodge that lets the player slip through a gap is this number changing for a
/// few ticks. Neither needs a branch in the solver.
const PLAYER_INV_MASS: f32 = 0.05;
const ENEMY_INV_MASS: f32 = 1.0;
const _: () = assert!(PLAYER_INV_MASS > 0.0 && ENEMY_INV_MASS > 0.0);
const _: () = assert!(
    PLAYER_INV_MASS < ENEMY_INV_MASS,
    "an equal or lighter player is shoved around by fodder"
);

/// Below this separation two bodies have no line between them to push along,
/// and normalising their difference yields NaN — a position no clamp recovers.
/// Squared, since that is what the overlap test already has to hand.
const COINCIDENT_SQ: f32 = 1e-12;
const _: () = assert!(COINCIDENT_SQ > 0.0);

/// Half the ground plane's width, in world units.
const ARENA_HALF: f32 = GROUND_TILES as f32 * TILE * 0.5;
const _: () = assert!(ARENA_HALF > PLAYER_SCALE.x && ARENA_HALF > PLAYER_SCALE.z);

/// World units per second. Fast enough that the arena crosses in a few seconds,
/// which is the range an ARPG lives in — slow movement makes a horde feel like
/// a traffic jam rather than a threat.
const PLAYER_SPEED: f32 = 9.0;
const _: () = assert!(PLAYER_SPEED > 0.0);

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

/// Radians per second. Fast — a full 180° turn takes ~0.22s — because in an
/// ARPG the character reorienting is feedback that the input registered, and
/// anything slow enough to notice reads as the controls lagging.
const PLAYER_TURN_RATE: f32 = 14.0;
const _: () = assert!(PLAYER_TURN_RATE > 0.0);

/// Folds an angle into `(-PI, PI]`, so facing cannot drift off toward the
/// precision limit over a long session.
fn wrap_angle(radians: f32) -> f32 {
    let wrapped = radians.rem_euclid(std::f32::consts::TAU);
    if wrapped > std::f32::consts::PI { wrapped - std::f32::consts::TAU } else { wrapped }
}

/// The signed angle from `from` to `to`, **always the short way round**.
///
/// This is the whole subtlety in turning. Subtracting raw angles has a seam:
/// turning from 350° to 10° is a 20° step to the left, but `to - from` says
/// -340°, so the character spins almost all the way round the wrong way for
/// what looks on screen like a tiny correction. Wrapping the difference — not
/// the inputs — is what removes the seam.
fn shortest_arc(from: f32, to: f32) -> f32 {
    wrap_angle(to - from)
}

/// A separation direction for two bodies occupying exactly the same point.
///
/// It has to come from somewhere, and it has to be the *same* somewhere every
/// run: a random direction would make two identical simulations diverge, which
/// is precisely what the determinism the fixed timestep is for would be
/// claiming. Deriving it from the pair's index costs nothing and is exactly
/// reproducible.
///
/// Golden-angle steps rather than a fixed direction, so a clump of coincident
/// bodies fans out instead of every one of them being pushed the same way and
/// re-stacking on the next tick.
fn escape_direction(tiebreak: usize) -> Vec2 {
    /// `PI * (3 - sqrt(5))`, written out because `sqrt` is not const.
    const GOLDEN_ANGLE: f32 = 2.399_963_2;

    let angle = tiebreak as f32 * GOLDEN_ANGLE;
    Vec2::new(angle.cos(), angle.sin())
}

/// Pushes two overlapping bodies apart along the line joining them, splitting
/// the correction between them by inverse mass. Returns whether they touched.
///
/// **Position projection, not an impulse.** There is no velocity here and no
/// momentum to conserve, which is the right model for a game whose movement is
/// deliberately instantaneous: a solver whose whole job is conserving momentum
/// would be fighting that. It also cannot inject energy, so there is no
/// restitution to zero out and no explosive pushback to suppress.
///
/// The overlap is corrected in **full**, in one pass. The usual advice is to
/// resolve a fraction — Box2D uses 0.2 — but that reasoning is about oblong
/// shapes overshooting as they rotate, and these are discs that do not rotate
/// and cannot stack. More to the point, a fraction applied once a tick is a
/// per-tick lerp toward zero overlap, which is frame-rate dependent in exactly
/// the way [`arpg_core::damp`] exists to prevent: it would converge five times
/// faster uncapped than under vsync, so pressing `V` would change how the game
/// feels and corrupt the measurement `V` is for. Full correction is exactly
/// dt-independent, and it keeps that question shut until the fixed timestep
/// makes it answerable.
fn separate(
    a: &mut Vec2,
    b: &mut Vec2,
    contact_distance: f32,
    a_inv_mass: f32,
    b_inv_mass: f32,
    tiebreak: usize,
) -> bool {
    let delta = *b - *a;
    let gap_sq = delta.length_squared();
    if gap_sq >= contact_distance * contact_distance {
        return false;
    }

    // Two immovable bodies have no correction to share out. Bailing keeps the
    // division below from being a zero-divide that quietly yields NaN.
    let share = a_inv_mass + b_inv_mass;
    if share <= 0.0 {
        return false;
    }

    let (normal, gap) = if gap_sq > COINCIDENT_SQ {
        let gap = gap_sq.sqrt();
        (delta / gap, gap)
    } else {
        (escape_direction(tiebreak), 0.0)
    };

    let overlap = contact_distance - gap;
    *a -= normal * (overlap * a_inv_mass / share);
    *b += normal * (overlap * b_inv_mass / share);
    true
}

/// Where the horde is.
///
/// Until now an enemy had no position. `extract_enemies` derived one from the
/// loop index on the way to the GPU and threw it away, which made the horde a
/// *drawing* rather than a thing — there was nothing for a shove to move,
/// because there was nothing there between frames. Storing it is the whole
/// content of this step, and every interaction downstream needs it first.
///
/// Structure-of-arrays rather than `Vec<Enemy>`, decided now while there is
/// nothing to migrate. The broadphase that lands next walks positions and
/// nothing else, and a contiguous stream is what it wants; fields an enemy
/// gains later — health, AI state, cooldowns — belong in their own arrays
/// beside this one, so the hot loop never drags them through cache on its way
/// to a position it does want.
#[derive(Default)]
struct Enemies {
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
}

impl Enemies {
    fn len(&self) -> usize {
        self.pos.len()
    }

    /// Lays `n` enemies out in a square grid centred on the origin.
    ///
    /// Respawns the whole horde rather than appending to it, which keeps `[`
    /// and `]` behaving exactly as they did: the layout is a function of N, so
    /// halving it re-centres what is left rather than deleting a corner.
    /// Enemies that persist across a count change is the better model and it is
    /// what real spawning will want — but it is a *spawning* decision, and
    /// smuggling it in beside the storage change would mean this step could no
    /// longer be checked by the picture staying identical.
    fn respawn(&mut self, n: usize) {
        // `clear` keeps the allocation, so doubling N repeatedly grows the
        // buffer a few times rather than reallocating on every press.
        self.pos.clear();
        self.pos.reserve(n);

        let side = (n as f32).sqrt().ceil().max(1.0) as usize;
        let offset = (side as f32 - 1.0) * ENEMY_SPACING * 0.5;

        for i in 0..n {
            self.pos.push(Vec2::new(
                (i % side) as f32 * ENEMY_SPACING - offset,
                (i / side) as f32 * ENEMY_SPACING - offset,
            ));
        }
    }
}

/// The player-controlled character.
///
/// A single struct held apart from the horde, and it stays that way even once
/// enemy storage becomes SoA arrays: there is exactly one of these, it is the
/// only thing input drives, and it will accumulate state no enemy has — facing,
/// attack phase, i-frames, buffered inputs. Wedging it into the horde's storage
/// to avoid a "special case" would mean paying for those fields N times.
#[derive(Default)]
struct Player {
    /// Ground-plane position, on the same terms as the horde's: the height a
    /// body is drawn at is a constant of its size, so there is no Y here for
    /// movement to leave the floor through. That used to be a unit test; it is
    /// now unrepresentable, which is where an invariant belongs.
    pos: Vec2,
    /// Which way the body points, in radians. Yaw 0 faces world +Z and positive
    /// turns toward +X, matching `Instance::with_yaw` and `shader.wgsl`.
    ///
    /// Simulation state, not a rendering detail: this is what the attack hitbox
    /// will be oriented by, so it has to be something the sim owns and can be
    /// reasoned about without a GPU.
    facing: f32,
}

/// What exists. This is where the simulation will live as it grows —
/// fixed-timestep stepping, entity storage, spatial partitioning.
pub struct World {
    enemies: Enemies,
    player: Player,
    /// Contacts resolved by the last [`World::step`].
    ///
    /// The instrument the collision work is measured with, and it exists
    /// because the alternative was reading pixels: a body is nine pixels across
    /// at this zoom and a contact displaces it by a fraction of that, so
    /// "is anything actually touching" is invisible on screen and obvious as a
    /// number. It earns its keep twice — when the broadphase lands, a grid that
    /// finds a different number of contacts than brute force is wrong, and this
    /// is how that gets caught.
    contacts: usize,
}

impl Default for World {
    fn default() -> Self {
        let mut world =
            Self { enemies: Enemies::default(), player: Player::default(), contacts: 0 };
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
    pub fn set_enemy_count(&mut self, n: usize) {
        self.enemies.respawn(n.clamp(1, MAX_ENEMIES));
    }

    /// Advances the world by `dt` seconds.
    ///
    /// `dt` is the raw frame time and movement is integrated against it, so
    /// speed is frame-rate independent today. It is a plain `f32` rather than
    /// the `Dt` newtype the roadmap wants, and deliberately so: the whole point
    /// of `Dt` is that it is *fixed*, and minting one from a variable frame time
    /// would be a type asserting something untrue. It arrives with the
    /// fixed-timestep accumulator, not before.
    ///
    /// `move_dir` is world-space and already unit-or-zero — the type says so,
    /// so this does not have to check.
    pub fn step(&mut self, dt: f32, move_dir: MoveDir) {
        self.player.pos += move_dir.as_vec3().xz() * PLAYER_SPEED * dt;

        // Order matters, and this is the whole of it: move first, then push
        // bodies out of each other, then put everything back inside the world.
        // Clamping before resolving would let a contact shove a body through
        // the wall and leave it there until something else happened to touch
        // it again.
        self.resolve_contacts();
        self.clamp_to_arena();

        self.turn_toward(move_dir, dt);
    }

    /// Separates every pair of bodies that overlap.
    ///
    /// Brute force, deliberately, and only against the player for now. It is
    /// O(N) at this point, so at the default horde it is a thousand distance
    /// checks a tick and costs nothing worth measuring. The uniform grid this
    /// wants eventually is an *optimisation of something already correct* —
    /// which means it can be tested by agreeing with this, and that test only
    /// exists if this exists first.
    ///
    /// Gauss-Seidel: each correction is written immediately, so the next pair
    /// sees it. That converges faster per pass than accumulating and applying
    /// at the end, and its one real cost — the result depends on the order
    /// pairs are visited — is fine here because the order is a fixed walk over
    /// storage rather than anything that varies run to run.
    fn resolve_contacts(&mut self) {
        let contact = PLAYER_RADIUS + ENEMY_RADIUS;
        // Disjoint fields, so both may be borrowed mutably at once.
        let player = &mut self.player.pos;
        let mut contacts = 0;

        for (i, enemy) in self.enemies.pos.iter_mut().enumerate() {
            contacts +=
                usize::from(separate(player, enemy, contact, PLAYER_INV_MASS, ENEMY_INV_MASS, i));
        }

        self.contacts = contacts;
    }

    /// Keeps every body inside the world.
    ///
    /// The ground plane *is* the world; there is nothing beyond it to walk
    /// onto. Enemies need this now for the first time — they have never moved
    /// before, and without it a horde being shoved outward slowly walks off the
    /// floor.
    ///
    /// It also hands over a mechanic for free. A body clamped against the wall
    /// cannot yield along that axis, so it backs up the bodies behind it, and
    /// pinning a crowd against terrain starts working without anyone
    /// implementing it.
    fn clamp_to_arena(&mut self) {
        let player_limit = Vec2::splat(ARENA_HALF - PLAYER_RADIUS);
        self.player.pos = self.player.pos.clamp(-player_limit, player_limit);

        let enemy_limit = Vec2::splat(ARENA_HALF - ENEMY_RADIUS);
        for pos in &mut self.enemies.pos {
            *pos = pos.clamp(-enemy_limit, enemy_limit);
        }
    }

    /// Rotates the body toward where it is heading, at a fixed rate.
    ///
    /// A fixed rate rather than the exponential damping the camera uses. Both
    /// are frame-rate independent, but exponential decay is asymptotic — it
    /// crawls through the last few degrees and never quite arrives, which on a
    /// body reads as drifting rather than turning. A constant rate arrives, and
    /// it makes the behaviour a number you can state: 180° in `PI / rate`
    /// seconds. Clamping the step to the remaining arc is what keeps it from
    /// overshooting and oscillating around the target.
    fn turn_toward(&mut self, move_dir: MoveDir, dt: f32) {
        let dir = move_dir.as_vec3();
        if dir == Vec3::ZERO {
            // Standing still keeps the last facing. Snapping back to a default
            // would have the character turn away from whatever it just walked
            // up to the instant the key came up.
            return;
        }

        let arc = shortest_arc(self.player.facing, f32::atan2(dir.x, dir.z));
        let step = (PLAYER_TURN_RATE * dt).min(arc.abs());
        self.player.facing = wrap_angle(self.player.facing + step * arc.signum());
    }

    /// Where the character is standing, lifted to world space for the camera
    /// and the renderer. Stored on the ground plane; the height is a constant
    /// of the body's size rather than state.
    pub fn player_pos(&self) -> Vec3 {
        on_ground(self.player.pos, PLAYER_HALF_HEIGHT)
    }

    /// Which way the character is pointing, in radians. The attack state
    /// machine will orient its hitbox by this.
    pub fn player_facing(&self) -> f32 {
        self.player.facing
    }

    /// How many overlapping pairs the last step pushed apart.
    pub fn contacts(&self) -> usize {
        self.contacts
    }

    /// **The seam.** The world describes itself in the renderer's vocabulary;
    /// `gfx` never sees a `World`.
    ///
    /// Takes the sink by value, so it is single-use and cannot outlive the
    /// frame. Everything about the buffer — that it was reset, that it is
    /// capacity-bounded, that pushing is the only thing anyone may do to it —
    /// is settled by the type rather than by remembering. Next chunk this
    /// grows an `alpha` parameter for interpolating between simulation ticks.
    pub fn extract(&self, mut out: InstanceSink<'_>) {
        self.extract_ground(&mut out);
        self.extract_enemies(&mut out);
        self.extract_player(&mut out);
    }

    /// The player is not a special case to the renderer either — one more cube
    /// in the same draw call. Only the colour and the silhouette distinguish it.
    fn extract_player(&self, out: &mut InstanceSink<'_>) {
        // Linear, and it looks wrong here on purpose: the surface is sRGB, so
        // the hardware encodes on write. This is roughly sRGB (0.35, 0.72, 0.95)
        // — a bright cyan-blue, chosen to sit opposite the horde's muted red on
        // the colour wheel so the eye separates them without effort.
        out.push(
            Instance::new(self.player_pos(), PLAYER_SCALE, Vec3::new(0.10, 0.47, 0.88))
                .with_yaw(self.player.facing),
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

    /// Reads the horde's stored positions rather than re-deriving them, which
    /// is the whole difference this step makes: what is drawn is now what the
    /// simulation believes, so moving a body moves its cube.
    fn extract_enemies(&self, out: &mut InstanceSink<'_>) {
        for (i, &pos) in self.enemies.pos.iter().enumerate() {
            // Linear-space colour, since the surface is sRGB and the hardware
            // encodes on write. These look darker here than they will on screen.
            let t = (i % 7) as f32 / 7.0;
            let color = Vec3::new(0.30 + t * 0.12, 0.06 + t * 0.05, 0.05);

            out.push(Instance::new(on_ground(pos, ENEMY_HALF_HEIGHT), ENEMY_SCALE, color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::InstanceBuffer;

    /// A world whose player is clear of every body, so a test can measure
    /// movement without measuring contact.
    ///
    /// Needed from the moment bodies touch: the horde spawns centred on the
    /// origin and so does the player, which puts the two in contact on the
    /// very first tick. Anything asking a question about *movement* has to get
    /// out of the crowd first, or it is really asking about the solver.
    fn in_open_ground() -> World {
        let mut world = World::default();
        world.set_enemy_count(1);

        // The lone body spawns on top of the player. Two seconds east at
        // `PLAYER_SPEED` clears it by 18 units.
        for _ in 0..120 {
            world.step(1.0 / 60.0, MoveDir::new(Vec3::X));
        }
        world
    }

    /// Speed must come from `dt`, not from how often `step` happens. Two half
    /// steps and one whole step have to land in the same place, or the game
    /// runs faster on a faster machine — the oldest bug in the medium.
    ///
    /// This is about the *integrator*, so it is measured in open ground.
    /// Walking through a crowd is a different question and has a different
    /// answer: contacts are resolved once per tick, so a step count that
    /// depends on frame rate resolves a different number of them. That is the
    /// bill the fixed timestep is there to pay, and it is not yet paid.
    #[test]
    fn movement_is_frame_rate_independent() {
        let east = MoveDir::new(Vec3::X);

        let mut coarse = in_open_ground();
        coarse.step(0.2, east);

        let mut fine = in_open_ground();
        for _ in 0..20 {
            fine.step(0.01, east);
        }

        assert!((coarse.player_pos() - fine.player_pos()).length() < 1e-4);
    }

    #[test]
    fn no_input_does_not_move_the_player() {
        let mut world = in_open_ground();
        let start = world.player_pos();
        world.step(1.0, MoveDir::NONE);
        assert_eq!(world.player_pos(), start);
    }

    /// Walking into the wall must stop, not leave the ground plane — and must
    /// stay finite, since a NaN position would silently vanish the character.
    #[test]
    fn the_player_cannot_walk_off_the_arena() {
        let mut world = World::default();
        for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
            for _ in 0..100 {
                world.step(1.0, MoveDir::new(dir));
            }
            let pos = world.player_pos();
            assert!(pos.is_finite());
            assert!(pos.x.abs() <= ARENA_HALF && pos.z.abs() <= ARENA_HALF, "escaped: {pos}");
        }
    }

    /// **The seam that turning exists to get right.** Crossing the ±PI branch
    /// cut must be a small step, not an almost-full revolution the other way.
    #[test]
    fn turning_takes_the_short_way_around() {
        let nearly_half_turn = std::f32::consts::PI - 0.1;
        let just_past = -nearly_half_turn;

        let arc = shortest_arc(nearly_half_turn, just_past);
        assert!(arc.abs() < 0.3, "went the long way: {arc}");

        // And the naive subtraction this replaces really does get it wrong,
        // which is why the wrapping is not decoration.
        assert!((just_past - nearly_half_turn).abs() > 6.0);
    }

    #[test]
    fn facing_follows_the_direction_of_travel() {
        let mut world = World::default();
        for _ in 0..120 {
            world.step(1.0 / 60.0, MoveDir::new(Vec3::X));
        }
        // atan2(dir.x, dir.z): due east is +X, so a quarter turn from +Z.
        assert!((world.player.facing - std::f32::consts::FRAC_PI_2).abs() < 1e-4);

        for _ in 0..120 {
            world.step(1.0 / 60.0, MoveDir::new(Vec3::Z));
        }
        assert!(world.player.facing.abs() < 1e-4, "should face +Z");
    }

    /// A fixed turn rate is only frame-rate independent if the step is clamped
    /// to the remaining arc; without the clamp the coarse step overshoots and
    /// the two disagree.
    #[test]
    fn turning_is_frame_rate_independent() {
        let west = MoveDir::new(Vec3::NEG_X);

        let mut coarse = World::default();
        coarse.step(0.05, west);

        let mut fine = World::default();
        for _ in 0..5 {
            fine.step(0.01, west);
        }

        assert!((coarse.player.facing - fine.player.facing).abs() < 1e-5);
    }

    #[test]
    fn turning_never_overshoots_its_target() {
        let mut world = World::default();
        let target = std::f32::consts::FRAC_PI_2;

        for _ in 0..200 {
            world.step(1.0 / 60.0, MoveDir::new(Vec3::X));
            assert!(world.player.facing >= 0.0);
            assert!(world.player.facing <= target, "overshot to {}", world.player.facing);
        }
    }

    /// Releasing the keys must not reorient the character — it would turn away
    /// from whatever it just walked up to.
    #[test]
    fn standing_still_keeps_the_last_facing() {
        let mut world = World::default();
        for _ in 0..120 {
            world.step(1.0 / 60.0, MoveDir::new(Vec3::NEG_Z));
        }
        let settled = world.player.facing;

        for _ in 0..120 {
            world.step(1.0 / 60.0, MoveDir::NONE);
        }
        assert_eq!(world.player.facing, settled);
    }

    /// Facing must stay canonical however long the session runs, rather than
    /// accumulating toward the range where f32 loses angular precision.
    #[test]
    fn facing_stays_wrapped_while_spinning() {
        let mut world = World::default();
        let circle = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z];

        for lap in 0..50 {
            for _ in 0..30 {
                world.step(1.0 / 60.0, MoveDir::new(circle[lap % 4]));
            }
            assert!(
                world.player.facing.abs() <= std::f32::consts::PI + 1e-6,
                "drifted to {}",
                world.player.facing
            );
        }
    }

    /// The whole world — floor, horde and player — has to fit the one buffer
    /// they share, at the largest horde the clamp permits.
    #[test]
    fn a_full_horde_still_fits_alongside_the_ground_and_the_player() {
        let mut world = World::default();
        world.set_enemy_count(usize::MAX);

        let mut buf = InstanceBuffer::default();
        world.extract(buf.sink());

        assert_eq!(buf.as_slice().len(), MAX_INSTANCES);
    }

    /// The count is derived from the storage, so asking for N must actually
    /// produce N bodies — not N draw calls over a formula.
    #[test]
    fn the_horde_holds_exactly_the_requested_count() {
        let mut world = World::default();
        assert_eq!(world.enemy_count(), DEFAULT_ENEMIES);

        for n in [1, 17, 512, 1024, 4096] {
            world.set_enemy_count(n);
            assert_eq!(world.enemy_count(), n);
            assert_eq!(world.enemies.pos.len(), n);
        }
    }

    /// Nothing may spawn already overlapping.
    ///
    /// The const assert beside `ENEMY_SPACING` covers the constants; this
    /// covers the *layout* they produce, which is the thing that actually has
    /// to hold. Once bodies push each other apart, an interpenetrated spawn
    /// resolves every overlap on frame one and detonates the horde — a failure
    /// that looks like a physics bug and is a spawning bug.
    #[test]
    fn the_horde_spawns_with_a_gap_between_every_body() {
        let mut world = World::default();
        world.set_enemy_count(1024);

        let pos = &world.enemies.pos;
        let mut closest = f32::MAX;
        for i in 0..pos.len() {
            for j in i + 1..pos.len() {
                closest = closest.min(pos[i].distance(pos[j]));
            }
        }

        assert!(
            closest > ENEMY_SCALE.x,
            "spawned {closest} apart, but a body is {} wide",
            ENEMY_SCALE.x
        );
    }

    /// The grid is centred on the origin, which is what puts the player inside
    /// the horde rather than beside it.
    ///
    /// Exactly centred only when N is a perfect square. Otherwise the last row
    /// is partial and drags the centroid by up to one spacing — which is the
    /// real behaviour and worth pinning at that bound rather than pretending
    /// the grid is always square.
    #[test]
    fn the_horde_is_centred_on_the_origin() {
        let mut world = World::default();

        let centroid_at = |world: &World| {
            let pos = &world.enemies.pos;
            pos.iter().fold(Vec2::ZERO, |acc, &p| acc + p) / pos.len() as f32
        };

        for n in [1, 4, 1024] {
            world.set_enemy_count(n);
            let c = centroid_at(&world);
            assert!(c.length() < 1e-3, "square N={n} should be exactly centred, got {c}");
        }

        for n in [17, 500, 4095] {
            world.set_enemy_count(n);
            let c = centroid_at(&world);
            assert!(c.length() < ENEMY_SPACING, "ragged N={n} drifted {c}, more than one row");
        }
    }

    /// The one place `Vec2::y` means world Z, so it is worth pinning: a
    /// transposition here is horizontal either way and would draw the whole
    /// horde mirrored along a diagonal without a single test failing elsewhere.
    #[test]
    fn a_ground_position_keeps_x_and_lifts_y_into_z() {
        assert_eq!(on_ground(Vec2::new(3.0, -7.0), 0.25), Vec3::new(3.0, 0.25, -7.0));
    }

    /// What separation is *for*: afterwards the two are exactly touching, not
    /// merely less overlapped.
    #[test]
    fn separating_leaves_two_bodies_exactly_touching() {
        let mut a = Vec2::new(-0.1, 0.0);
        let mut b = Vec2::new(0.1, 0.0);

        assert!(separate(&mut a, &mut b, 1.0, 1.0, 1.0, 0));
        assert!((a.distance(b) - 1.0).abs() < 1e-5, "settled at {}", a.distance(b));
    }

    /// Bodies that are merely near must not be touched at all, or the solver
    /// would jitter everything within reach of everything.
    #[test]
    fn bodies_that_do_not_overlap_are_left_alone() {
        let mut a = Vec2::new(-1.0, 0.0);
        let mut b = Vec2::new(1.0, 0.0);

        assert!(!separate(&mut a, &mut b, 1.0, 1.0, 1.0, 0));
        assert_eq!((a, b), (Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0)));
    }

    /// **The case that produces NaN if it is not handled.** Two bodies at the
    /// same point have no line between them, and normalising their difference
    /// poisons a position beyond any clamp's ability to recover. It happens for
    /// real: a crowd converging on one target arrives at one point.
    #[test]
    fn coincident_bodies_separate_instead_of_producing_nan() {
        let mut a = Vec2::new(5.0, -3.0);
        let mut b = Vec2::new(5.0, -3.0);

        assert!(separate(&mut a, &mut b, 1.0, 1.0, 1.0, 7));

        assert!(a.is_finite() && b.is_finite(), "poisoned: {a} {b}");
        assert!((a.distance(b) - 1.0).abs() < 1e-5);
    }

    /// The escape direction must be reproducible, or two identical runs
    /// diverge the first time anything lands on top of anything else.
    #[test]
    fn the_escape_from_a_coincident_pair_is_deterministic() {
        let run = || {
            let (mut a, mut b) = (Vec2::ZERO, Vec2::ZERO);
            separate(&mut a, &mut b, 1.0, 1.0, 1.0, 42);
            (a, b)
        };
        assert_eq!(run(), run());

        // And neighbouring pairs must not all escape the same way, or a clump
        // separates into a line and re-stacks on the next tick.
        assert!(escape_direction(3).distance(escape_direction(4)) > 0.1);
    }

    /// **The mechanic, at the level of one contact.** The heavy body barely
    /// moves; the light one does almost all the yielding. This is the whole of
    /// why a single enemy is a nudge rather than a wall.
    #[test]
    fn the_heavier_body_yields_less() {
        let mut player = Vec2::ZERO;
        let mut enemy = Vec2::new(0.5, 0.0);
        let (start_player, start_enemy) = (player, enemy);

        separate(&mut player, &mut enemy, 1.0, PLAYER_INV_MASS, ENEMY_INV_MASS, 0);

        let player_moved = player.distance(start_player);
        let enemy_moved = enemy.distance(start_enemy);
        assert!(
            enemy_moved > player_moved * 10.0,
            "expected a lopsided split, got {player_moved} vs {enemy_moved}"
        );
    }

    /// Projection moves bodies apart without moving the pair: the mass-weighted
    /// centre is unchanged. That is what distinguishes it from an impulse — it
    /// can only redistribute position, never inject energy, so there is no
    /// explosive pushback to suppress.
    #[test]
    fn separation_preserves_the_mass_weighted_centre() {
        let (ia, ib) = (PLAYER_INV_MASS, ENEMY_INV_MASS);
        let centre = |a: Vec2, b: Vec2| (a / ia + b / ib) / (1.0 / ia + 1.0 / ib);

        let mut a = Vec2::new(0.2, -0.1);
        let mut b = Vec2::new(-0.1, 0.2);
        let before = centre(a, b);

        separate(&mut a, &mut b, 1.0, ia, ib, 0);
        assert!((centre(a, b) - before).length() < 1e-5);
    }

    /// Two immovable bodies cannot be pushed apart, and asking must not divide
    /// by their combined zero and yield NaN. Corpses and props will be exactly
    /// this case.
    #[test]
    fn two_immovable_bodies_are_left_where_they_are() {
        let mut a = Vec2::ZERO;
        let mut b = Vec2::new(0.1, 0.0);

        assert!(!separate(&mut a, &mut b, 1.0, 0.0, 0.0, 0));
        assert_eq!((a, b), (Vec2::ZERO, Vec2::new(0.1, 0.0)));
    }

    /// **The invariant the pass exists to establish**, checked on the real
    /// world rather than on a pair: after a step, nothing is inside the player.
    #[test]
    fn no_enemy_is_left_overlapping_the_player() {
        let mut world = World::default();

        // Walk into the middle of the horde and keep going.
        for _ in 0..240 {
            world.step(1.0 / 60.0, MoveDir::new(Vec3::X));
        }

        let contact = PLAYER_RADIUS + ENEMY_RADIUS;
        let player = world.player.pos;
        for (i, &enemy) in world.enemies.pos.iter().enumerate() {
            let gap = player.distance(enemy);
            assert!(gap >= contact - 1e-4, "enemy {i} is {gap} from the player, needs {contact}");
        }
    }

    /// Walking through the horde must displace it. A player that leaves the
    /// crowd exactly as it found it is not colliding with anything, which is a
    /// failure the previous test cannot see — it passes trivially if nothing
    /// ever overlaps because nothing ever touches.
    #[test]
    fn walking_through_the_horde_displaces_it() {
        let mut world = World::default();
        let before = world.enemies.pos.clone();

        let mut ever_touched = 0;
        for _ in 0..240 {
            world.step(1.0 / 60.0, MoveDir::new(Vec3::X));
            ever_touched += world.contacts();
        }

        let moved = before
            .iter()
            .zip(&world.enemies.pos)
            .filter(|(a, b)| a.distance(**b) > 1e-4)
            .count();

        assert!(ever_touched > 0, "nothing was ever in contact");
        assert!(moved > 0, "the player walked straight through {} bodies", before.len());
    }

    /// The contact count has to mean something, or it is a comforting number
    /// that would keep reporting zero if the solver stopped working. Standing
    /// clear of everything is zero; standing inside the horde is not.
    #[test]
    fn the_contact_count_tracks_whether_anything_is_touching() {
        let mut clear = in_open_ground();
        clear.step(1.0 / 60.0, MoveDir::NONE);
        assert_eq!(clear.contacts(), 0, "nothing is near the player out here");

        // The horde is centred on the origin and so is the player, so the
        // spawn itself puts bodies in contact.
        let mut crowded = World::default();
        crowded.step(1.0 / 60.0, MoveDir::NONE);
        assert!(crowded.contacts() > 0, "spawned inside the horde and touched nothing");
    }

    /// Everything stays inside the world, including bodies that only moved
    /// because something shoved them.
    #[test]
    fn nothing_is_pushed_out_of_the_arena() {
        let mut world = World::default();
        world.set_enemy_count(256);

        for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
            for _ in 0..600 {
                world.step(1.0 / 60.0, MoveDir::new(dir));
            }
            for &enemy in &world.enemies.pos {
                assert!(enemy.is_finite(), "poisoned position {enemy}");
                assert!(
                    enemy.x.abs() <= ARENA_HALF && enemy.y.abs() <= ARENA_HALF,
                    "escaped to {enemy}"
                );
            }
        }
    }
}
