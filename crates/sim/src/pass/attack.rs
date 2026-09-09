//! The swing: a shape, a window of ticks, and what it does to what it touches.
//!
//! **An attack is two bodies touching, with a different answer.** The question
//! is the same one the crowd solver asks — [`crate::contact::between`], disc
//! against disc — and everything that makes this an attack rather than a shove
//! is in the response and in *when* the question is allowed to be asked.
//!
//! The signature is where that shows up, and it is worth reading twice.
//! `pass::separate` takes `&mut [Vec2]` because pushing is what it does; this
//! takes `&[Vec2]` plus an `ImpulseSink`. It can request momentum, but cannot
//! move a position or reach the rest of the physics store. The motion pass
//! integrates that momentum on the next tick.
//!
//! ## Why the window is the hard part
//!
//! Startup, active and recovery are the whole texture of a melee attack. An
//! attack that connects one tick early is not a bug you can see: the swing
//! plays, the enemy is hit, nothing crashes and the final state is the state a
//! correct swing would have left. It exists only in the interval between two
//! samples. Every timing claim below is therefore asserted against a golden
//! trace, which is the only artefact that can see an interval.

use super::health::DamageSink;
use super::motion::{Impulse, ImpulseSink};
use glam::Vec2;

use crate::contact;

use crate::swing::{Disc, Hitbox, Swing};
use crate::trace::{Event, TraceSink};
use crate::{
    AttackPhase, AttackProfile, AttackStatus, ENEMY_RADIUS, EntityId, RecoveryTicks, ResolvedAttack,
};

/// How many discs a hitbox is made of: one per tick of the active window.
///
/// Public to the crate because the instance budget depends on it — `World` has
/// to reserve room to *draw* a swing, and a reservation that had to be kept in
/// step by hand would overrun the GPU buffer in silence.
pub(crate) const HITBOX_SAMPLES: usize = ResolvedAttack::MAX_ACTIVE_TICKS as usize;

/// How many bodies one swing is expected to strike.
///
/// Only a starting capacity: the list grows if a swing lands in a crowd. It is
/// reserved up front so that a steady-state tick does not reach the allocator,
/// which is a property `a_steady_state_frame_allocates_nothing` checks.
const EXPECTED_HITS: usize = 16;

/// A swing in the air: how far through it is, and the shape it committed to.
///
/// **The two are one value, so a hitbox cannot exist without a timer to say
/// which part of it is live.** Kept as separate fields on [`Attack`] they were
/// two things that had to agree, and nothing but convention said a caller had
/// to check the timer before reading the shape.
pub(crate) struct InFlight {
    /// Ticks since the swing began. Names the tick that just ran — see
    /// [`attack`] for why it is advanced before anything reads it.
    elapsed: u32,
    /// Authored move identity captured at the same boundary as its resolved values.
    profile: AttackProfile,
    /// Captured at the press; later resolution cannot reshape this swing.
    resolved: ResolvedAttack,
    /// Where this swing's hitbox goes, in the player's own frame.
    ///
    /// **Generated when the swing starts, not recomputed per tick.** That is
    /// what makes it a value two readers can share — see [`crate::swing`] — and
    /// it is also the right moment for the choice: a weapon swapped mid-swing
    /// must not reshape the swing already in the air. This is the generated
    /// value chosen from the configuration captured beside it.
    ///
    /// Local rather than world, because baking world positions at the press
    /// would bake the facing with them, and the player may still turn
    /// mid-swing.
    hitbox: Hitbox<HITBOX_SAMPLES>,
}

impl InFlight {
    /// Which part of the swing this tick falls in.
    ///
    /// **Each phase carries what that phase has**, rather than a bare tag
    /// beside three numbers of which two are always dead. It also means the
    /// live disc's index reaches its readers from the same place that decided
    /// the window, so `strike` and `extract` cannot disagree about which disc
    /// is hot — there is no second derivation to drift.
    pub(crate) fn phase(&self) -> Phase {
        let startup = self.resolved.startup();
        if self.elapsed < startup {
            Phase::Startup(self.elapsed as f32 / startup as f32)
        } else if is_active(self.elapsed, self.resolved) {
            Phase::Active((self.elapsed - startup) as usize)
        } else {
            Phase::Recovery
        }
    }

    /// Every disc this swing occupies, in the order it occupies them.
    pub(crate) fn discs(&self) -> &[Disc] {
        self.hitbox.discs()
    }
}

/// Where the swing has got to.
///
/// **One `Option`, with every phase derived from it**, rather than a stored
/// phase beside a counter. Two fields that must agree are two fields that can
/// disagree, and the disagreement here would be a hitbox live during recovery —
/// silent, and visible only as an attack that feels wrong.
pub(crate) struct Attack {
    /// The swing in the air, or `None` when idle.
    swing: Option<InFlight>,
    /// Authored move selected for the next swing.
    profile: AttackProfile,
    /// The next swing's complete configuration; the current swing owns a copy.
    resolved: ResolvedAttack,
    /// Bodies this swing has already struck.
    ///
    /// **Once per swing, not once per tick.** Without this, a body standing in
    /// the hitbox takes one hit for every configured active tick — which is not
    /// a tuning problem, it is a different game.
    ///
    /// A linear scan, deliberately: it holds what one swing hit, which is a
    /// handful, and a set would cost more to build each swing than the scan
    /// saves. If a swing ever hits hundreds, this is the line to revisit.
    struck: Vec<EntityId>,
}

impl Default for Attack {
    fn default() -> Self {
        Self {
            swing: None,
            profile: AttackProfile::default(),
            resolved: AttackProfile::default().resolve(),
            struck: Vec::with_capacity(EXPECTED_HITS),
        }
    }
}

impl Attack {
    /// Whether a swing is in progress.
    #[cfg(test)]
    pub(crate) fn is_swinging(&self) -> bool {
        self.swing.is_some()
    }

    /// A fresh observation, never a second maintained copy of attack state.
    pub(crate) fn status(&self) -> AttackStatus {
        let Self { swing, profile, resolved, struck } = self;
        let phase = match swing.as_ref().map(InFlight::phase) {
            None => AttackPhase::Idle,
            Some(Phase::Startup(_)) => AttackPhase::Startup,
            Some(Phase::Active(_)) => AttackPhase::Active,
            Some(Phase::Recovery) => AttackPhase::Recovery,
        };
        AttackStatus {
            phase,
            elapsed: swing.as_ref().map_or(0, |s| s.elapsed),
            profile: *profile,
            swing_profile: swing.as_ref().map(|s| s.profile),
            resolved: *resolved,
            swing_resolved: swing.as_ref().map(|s| s.resolved),
            recovery: resolved.recovery(),
            swing_recovery: swing.as_ref().map(|s| s.resolved.recovery()),
            struck: struck.len(),
        }
    }

    pub(crate) fn set_profile(&mut self, profile: AttackProfile) -> bool {
        let resolved = profile.resolve();
        let changed = self.profile != profile || self.resolved != resolved;
        self.profile = profile;
        self.resolved = resolved;
        changed
    }

    pub(crate) fn set_recovery(&mut self, recovery: RecoveryTicks) -> bool {
        let changed = self.resolved.recovery() != recovery;
        self.resolved = self.resolved.with_recovery(recovery);
        changed
    }

    /// Whether the hitbox exists right now.
    pub(crate) fn hitbox_is_live(&self) -> bool {
        matches!(self.swing.as_ref().map(InFlight::phase), Some(Phase::Active(_)))
    }

    /// The swing in the air, for anything that needs its shape as well as its
    /// timer. `None` when idle, which is what makes reading one without the
    /// other impossible rather than merely discouraged.
    pub(crate) fn in_flight(&self) -> Option<&InFlight> {
        self.swing.as_ref()
    }

    /// How many bodies the current or most recent swing struck.
    pub(crate) fn struck(&self) -> usize {
        self.struck.len()
    }

    /// Feeds the swing into the world hash. Exhaustive, as every hash here is.
    pub(crate) fn hash(&self, h: &mut crate::hash::Fnv) {
        let Self { swing, profile, resolved, struck } = self;
        h.usize(*profile as usize);
        resolved.hash(h);

        // `None` and "tick 0 of a swing" are different states and must hash
        // differently, so the discriminant goes in as well as the value.
        h.usize(usize::from(swing.is_some()));
        if let Some(InFlight { elapsed, profile, resolved, hitbox }) = swing {
            h.u64(u64::from(*elapsed));
            h.usize(*profile as usize);
            resolved.hash(h);
            hitbox.hash(h);
        }

        h.usize(struck.len());
        for id in struck {
            id.hash_into(h);
        }
    }
}

/// Which part of the swing a tick falls in, and what that part has.
///
/// The three configured phases, as a thing a caller can match on
/// rather than three comparisons it has to get the boundaries right on.
pub(crate) enum Phase {
    /// Committed, and no hitbox yet. The wind-up, and how far through it, in
    /// `0.0..1.0` — which is what lets a telegraph brighten as it commits.
    Startup(f32),
    /// The hitbox exists, and this is which of its discs is live.
    Active(usize),
    /// The hitbox is gone, and no new swing may start yet.
    Recovery,
}

/// Whether the hitbox is live on this tick of the swing.
///
/// Half-open: live on `startup`, dead on `startup + active`. Written once, here,
/// so the open and close edges cannot drift apart — which is the classic way a
/// window ends up one tick wider than its configuration says.
fn is_active(elapsed: u32, resolved: ResolvedAttack) -> bool {
    (resolved.startup()..resolved.startup() + resolved.active()).contains(&elapsed)
}

/// Ground-plane pose shared by the hit query and the impulse's direction.
#[derive(Clone, Copy)]
pub(crate) struct Pose {
    pub(crate) pos: Vec2,
    pub(crate) facing: f32,
}

/// Advances the swing, and strikes whatever the hitbox is touching.
///
/// **Takes positions immutably**, which is the whole difference between this
/// pass and `pass::separate` given that both ask the same question of the same
/// geometry.
///
/// Runs last in the schedule, after `pass::face`, so the hitbox is oriented by
/// the facing this tick ended with rather than the one it started with. The
/// player may still turn mid-swing, and that is deliberate for now: locking
/// facing during an attack is a feel decision worth making against a real
/// animation rather than in advance.
///
/// The hitbox itself is generated at the press and stored — see
/// [`crate::swing`]. That is what lets `World::extract` draw the shape this
/// pass tests rather than a second opinion about where the sword is.
///
/// A press arriving while a swing is in progress is **dropped**, not queued.
/// Buffering it is a real feature and a separate one — it needs an expiry and a
/// rule about which phase accepts it, and guessing those now would bake in a
/// feel nobody has tried.
#[expect(
    clippy::too_many_arguments,
    reason = "positions and ids are one immutable body view, while impulse and damage sinks deliberately give the attack two independent effects without exposing either store"
)]
pub(crate) fn attack(
    state: &mut Attack,
    pose: Pose,
    pressed: bool,
    bodies: &[EntityId],
    pos: &[Vec2],
    mut impulses: ImpulseSink<'_>,
    mut damage: DamageSink<'_>,
    mut trace: TraceSink<'_>,
) {
    // **Advance first, then accept a press, then act.** The order is the whole
    // state machine, and the obvious alternative — advancing at the end — is
    // wrong in a way that only shows up when something *observes* the swing.
    //
    // Advancing last leaves `elapsed` pointing at the tick that has not run
    // yet, so `hitbox_is_live` read after `World::step` answers about the
    // future: it says the hitbox is live on a tick where nothing has been
    // struck, and dead on the tick where something was. The harness and the
    // report both read exactly there. Retiring the swing up here instead means
    // `elapsed` names the tick just processed, which is what every observer
    // assumes and what `tick` itself means.
    //
    // Retiring *before* the press check is also what makes the tick after
    // recovery the first that can start a new swing, rather than the one after
    // that.
    if let Some(swing) = &mut state.swing {
        swing.elapsed += 1;
    }
    if state
        .swing
        .as_ref()
        .is_some_and(|swing| {
            let resolved = swing.resolved;
            swing.elapsed >=
                resolved.startup() + resolved.active() + resolved.recovery().get()
        })
    {
        state.swing = None;
    }

    if state.swing.is_none() && pressed {
        let profile = state.profile;
        let resolved = state.resolved;
        let path = Swing::new(
            resolved.start(),
            resolved.end(),
            (resolved.start_radius(), resolved.end_radius()),
        );
        state.swing = Some(InFlight {
            elapsed: 0,
            profile,
            resolved,
            hitbox: path.generate(resolved.active() as usize),
        });
        state.struck.clear();
        trace.emit(Event::Swung);
    }

    let Some(swing) = &state.swing else { return };
    let elapsed = swing.elapsed;

    // Taken while the swing is only borrowed, so `strike` can have the `struck`
    // list mutably. `Disc` is `Copy` and twelve bytes, so this is a register
    // move rather than a compromise.
    let live = match swing.phase() {
        Phase::Active(sample) => Some(swing.hitbox.at(sample)),
        Phase::Startup(_) | Phase::Recovery => None,
    };

    let resolved = swing.resolved;
    if elapsed == resolved.startup() {
        trace.emit(Event::HitboxOpened);
    }

    if let Some(disc) = live {
        strike(
            &mut state.struck,
            disc,
            pose,
            resolved.knockback(),
            bodies,
            pos,
            &mut impulses,
            &mut damage,
            &mut trace,
        );
    }

    // Symmetric with the open above: `opened` on the first tick the hitbox
    // exists, `closed` on the first tick it does not. Emitting `closed` on the
    // *last* live tick instead would read as a one-tick-shorter window in every
    // trace, which is precisely the confusion these events exist to prevent.
    // Both land inside the swing because recovery is at least one tick.
    if elapsed == resolved.startup() + resolved.active() {
        trace.emit(Event::HitboxClosed);
    }
}

/// Tests one disc of the hitbox against every body and records what it touches.
///
/// **Handed the disc rather than working out which one is live**, which is what
/// makes "this cannot run outside the active window" a fact about the signature
/// instead of an assert: there is no index here to get wrong and no fallback to
/// silently pick the wrong disc. The caller has already matched on the phase
/// that produced it.
///
/// The disc it is handed is *this tick's*, not every disc the swing will
/// occupy. The hitbox sweeps: a body on the far side of an arc is struck later
/// than one on the near side, and that ordering is the whole texture of a
/// swing. A hitbox that accumulated its past positions would have the entire
/// arc live by the last active tick, which is the "a generous window reads as
/// mush" failure a short active window avoids, arriving through a
/// different door.
///
/// Takes `struck` rather than the whole `Attack` for the usual reason: it may
/// record what it hit and may not touch the timer or the shape.
#[expect(
    clippy::too_many_arguments,
    reason = "`bodies` and `pos` are one parallel pair and `knockback` is the one tuning value this may see; bundling them would hand `strike` more authority than taking positions immutably is meant to allow"
)]
fn strike(
    struck: &mut Vec<EntityId>,
    disc: Disc,
    pose: Pose,
    knockback: f32,
    bodies: &[EntityId],
    pos: &[Vec2],
    impulses: &mut ImpulseSink<'_>,
    damage: &mut DamageSink<'_>,
    trace: &mut TraceSink<'_>,
) {
    // The disc is *placed* rather than computed. `World::extract` places the
    // same one, from the same array, which is what makes the swing draw where
    // it hits — see `crate::swing`.
    let (centre, radius) = disc.place(pose.pos, pose.facing);
    let contact_distance = radius + ENEMY_RADIUS;

    // **Cheapest test first.** The distance check rejects almost every body in a
    // handful of instructions; the two checks that were above it — a load from
    // a second array, and a linear scan of what this swing has already hit —
    // then run only for the few that are actually in reach. Measured at 16384
    // bodies: 53µs a tick to 4µs.
    for (row, body) in pos.iter().enumerate() {
        if contact::between(centre, *body, contact_distance, row).is_none() {
            continue;
        }

        let id = bodies[row];
        if struck.contains(&id) {
            continue;
        }

        struck.push(id);
        let remaining = damage.hit(row);
        trace.emit(Event::Hit { id, remaining });
        let direction = Vec2::new(pose.facing.sin(), pose.facing.cos()) * knockback;
        impulses.push(
            id,
            Impulse::try_from((direction.x, direction.y)).expect("finite facing and tuning"),
            trace,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::Trace;

    /// The default resolved attack, which is what these tests pin: they check the state
    /// machine's shape against whatever it is configured with, not against a
    /// second copy of the numbers.
    fn resolved() -> ResolvedAttack {
        ResolvedAttack::default()
    }

    fn duration() -> u32 {
        let t = resolved();
        t.startup() + t.active() + t.recovery().get()
    }

    /// Drives a swing with no bodies to hit, so what is left is the state
    /// machine on its own. The scenarios cover it end to end; these localise a
    /// failure to the phases rather than to the geometry or the schedule.
    fn swing_for(ticks: u32, press_on: &[u32]) -> Vec<(u32, bool, bool)> {
        let mut state = Attack::default();
        let mut trace = Trace::default();
        let mut seen = Vec::new();
        let mut health = super::super::health::Health::default();
        let mut physics = super::super::motion::Physics::default();
        let player = crate::slots::Slots::default().insert();

        for tick in 0..ticks {
            attack(
                &mut state,
                Pose { pos: Vec2::ZERO, facing: 0.0 },
                press_on.contains(&tick),
                &[],
                &[],
                physics.sink(player),
                health.sink(),
                trace.sink(u64::from(tick)),
            );
            seen.push((tick, state.is_swinging(), state.hitbox_is_live()));
        }

        seen
    }

    /// The window's edges, stated as the resolved attack does. Half-open: live on
    /// `startup`, dead on `startup + active`.
    #[test]
    fn the_hitbox_is_live_for_exactly_the_active_ticks() {
        let t = resolved();
        let (startup, active) = (t.startup(), t.active());

        assert!(!is_active(startup - 1, t), "live during startup");
        assert!(is_active(startup, t), "not live on the first active tick");
        assert!(is_active(startup + active - 1, t), "not live on the last active tick");
        assert!(!is_active(startup + active, t), "still live after the window");

        let live: Vec<u32> = (0..duration()).filter(|&tick| is_active(tick, t)).collect();
        assert_eq!(live.len() as u32, active, "the window is not `active` ticks wide");
    }

    /// A swing occupies exactly its configured duration and then the player is idle
    /// again — and `elapsed` names the tick that just ran, so this reads the
    /// way the harness reads it.
    #[test]
    fn a_swing_lasts_exactly_its_length() {
        let seen = swing_for(duration() + 2, &[0]);

        for (tick, swinging, _) in &seen[..duration() as usize] {
            assert!(swinging, "idle on tick {tick}, inside the swing");
        }
        assert!(!seen[duration() as usize].1, "still swinging after its configured duration");
    }

    /// **Hitbox liveness is derived from one number, so it cannot disagree with
    /// the phase.** Checked against the resolved attack rather than a second
    /// copy of the schedule.
    #[test]
    fn the_hitbox_is_live_only_inside_the_swing() {
        for (tick, swinging, hitbox) in swing_for(duration() + 2, &[0]) {
            let expected = tick < duration() && is_active(tick, resolved());
            assert_eq!(hitbox, expected, "hitbox wrong on tick {tick}");
            if hitbox {
                assert!(swinging, "a live hitbox with no swing behind it, tick {tick}");
            }
        }
    }

    /// **The tunnelling limit, as a check rather than a paragraph.**
    ///
    /// One disc per tick ties the hitbox's spatial resolution to the tick rate,
    /// so a swing that travels far enough between two ticks leaves a gap and
    /// passes bodies straight through it. `crate::swing` explains why that is
    /// deferred; this is what stops the deferral being silent. A const assert
    /// would be higher up the ladder and is not available — the bound needs
    /// `atan2` and `sqrt`, neither of which is const on stable.
    ///
    /// Runtime configurations enforce this at their edit door; this local test
    /// keeps the default configuration honest as well.
    #[test]
    fn consecutive_discs_of_the_swing_leave_no_gap() {
        let t = resolved();
        let path = Swing::new(t.start(), t.end(), (t.start_radius(), t.end_radius()));
        let hitbox = path.generate::<HITBOX_SAMPLES>(t.active() as usize);
        let placed: Vec<(Vec2, f32)> =
            hitbox.discs().iter().map(|d| d.place(Vec2::ZERO, 0.0)).collect();

        for (i, pair) in placed.windows(2).enumerate() {
            let ((from, from_radius), (to, to_radius)) = (pair[0], pair[1]);

            // A body's centre between the two discs is caught by one of them
            // only while their reaches meet, and a body reaches out by its own
            // radius on each side.
            let covered = from_radius + to_radius + 2.0 * ENEMY_RADIUS;
            let gap = from.distance(to);

            assert!(
                gap <= covered,
                "discs {i} and {} are {gap} apart but cover {covered}: a body between them \
                 is passed straight through",
                i + 1
            );
        }
    }

    /// An idle player with the button down is not attacking. The edge is the
    /// caller's to supply, and this is what says the pass believes it.
    #[test]
    fn no_press_means_no_swing() {
        for (tick, swinging, hitbox) in swing_for(duration(), &[]) {
            assert!(!swinging && !hitbox, "swung with no press, tick {tick}");
        }
    }
}
