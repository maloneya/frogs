//! The swing: a shape, a window of ticks, and what it does to what it touches.
//!
//! **An attack is two bodies touching, with a different answer.** The question
//! is the same one the crowd solver asks — [`crate::contact::between`], disc
//! against disc — and everything that makes this an attack rather than a shove
//! is in the response and in *when* the question is allowed to be asked.
//!
//! The signature is where that shows up, and it is worth reading twice.
//! `pass::separate` takes `&mut [Vec2]` because pushing is what it does; this
//! takes `&[Vec2]`. A hitbox cannot move anything, and that is not a promise in
//! a comment — it is the absence of a `mut`, checked by the compiler at every
//! line of the pass.
//!
//! ## Why the window is the hard part
//!
//! Startup, active and recovery are the whole texture of a melee attack. An
//! attack that connects one tick early is not a bug you can see: the swing
//! plays, the enemy is hit, nothing crashes and the final state is the state a
//! correct swing would have left. It exists only in the interval between two
//! samples, which is exactly why `Trace` was built before the first hitbox
//! rather than after it. Every timing claim below is asserted against a golden
//! trace, because a golden trace is the only artefact that can see an interval.

use glam::Vec2;

use crate::contact;
use crate::slots::Slots;
use crate::trace::{Event, TraceSink};
use crate::{EntityId, ENEMY_RADIUS};

/// Ticks from the button to the hitbox appearing.
///
/// The cost of committing, and the thing that makes a swing readable to a
/// player rather than instantaneous. At 60Hz this is 100ms — long enough to see
/// the wind-up and step out of it, short enough not to feel like lag.
const STARTUP: u32 = 6;

/// Ticks the hitbox exists for. 67ms.
///
/// Short on purpose. A generous window forgives bad timing, which sounds kind
/// and reads as mush: if almost any moment connects, then *when* you swing
/// stops being a decision.
const ACTIVE: u32 = 4;

/// Ticks after the hitbox shuts before another swing may start. 167ms.
///
/// The commitment. Without it an attack is free and the correct play is to hold
/// the button down forever.
const RECOVERY: u32 = 10;

/// The whole swing, in ticks.
const SWING: u32 = STARTUP + ACTIVE + RECOVERY;

const _: () = assert!(STARTUP > 0, "a hitbox on the button frame cannot be reacted to");
const _: () = assert!(ACTIVE > 0, "a swing with no active window can never hit anything");
const _: () = assert!(RECOVERY > 0, "a swing with no recovery is free, so it is always correct");

/// How far in front of the player the hitbox sits, in world units.
const REACH: f32 = 1.1;

/// The hitbox's radius.
///
/// A disc, like every other shape in the simulation, and for the same reasons
/// [`crate::ENEMY_RADIUS`] gives — one unambiguous normal, no rebuild as the
/// player turns, and a test that is a squared-distance compare. A cone would
/// read better for a sword and is a change to this one function when the
/// silhouette starts mattering.
const HITBOX_RADIUS: f32 = 0.6;

const _: () = assert!(REACH > 0.0 && HITBOX_RADIUS > 0.0);
const _: () = assert!(
    REACH - HITBOX_RADIUS < crate::PLAYER_RADIUS + ENEMY_RADIUS,
    "a hitbox that starts beyond arm's length cannot hit a body already touching you"
);

/// How many bodies one swing is expected to strike.
///
/// Only a starting capacity: the list grows if a swing lands in a crowd. It is
/// reserved up front so that a steady-state tick does not reach the allocator,
/// which is a property `a_steady_state_frame_allocates_nothing` checks.
const EXPECTED_HITS: usize = 16;

/// Where the swing has got to.
///
/// **One number, with the phases derived from it**, rather than a stored phase
/// beside a counter. Two fields that must agree are two fields that can
/// disagree, and the disagreement here would be a hitbox live during recovery —
/// silent, and visible only as an attack that feels wrong.
pub(crate) struct Attack {
    /// Ticks since the swing began, or `None` when idle.
    elapsed: Option<u32>,
    /// Bodies this swing has already struck.
    ///
    /// **Once per swing, not once per tick.** The hitbox is live for four
    /// ticks, so without this a body standing in it takes four hits from one
    /// button press — which is not a tuning problem, it is a different game.
    ///
    /// A linear scan, deliberately: it holds what one swing hit, which is a
    /// handful, and a set would cost more to build each swing than the scan
    /// saves. If a swing ever hits hundreds, this is the line to revisit.
    struck: Vec<EntityId>,
}

impl Default for Attack {
    fn default() -> Self {
        Self { elapsed: None, struck: Vec::with_capacity(EXPECTED_HITS) }
    }
}

impl Attack {
    /// Whether a swing is in progress.
    pub(crate) fn is_swinging(&self) -> bool {
        self.elapsed.is_some()
    }

    /// Ticks since the swing began; 0 when idle.
    pub(crate) fn elapsed(&self) -> u32 {
        self.elapsed.unwrap_or(0)
    }

    /// Whether the hitbox exists right now.
    pub(crate) fn hitbox_is_live(&self) -> bool {
        self.elapsed.is_some_and(is_active)
    }

    /// How many bodies the current or most recent swing struck.
    pub(crate) fn struck(&self) -> usize {
        self.struck.len()
    }

    /// Feeds the swing into the world hash. Exhaustive, as every hash here is.
    pub(crate) fn hash(&self, h: &mut crate::hash::Fnv) {
        let Self { elapsed, struck } = self;

        // `None` and "tick 0 of a swing" are different states and must hash
        // differently, so the discriminant goes in as well as the value.
        h.usize(usize::from(elapsed.is_some()));
        h.u64(u64::from(elapsed.unwrap_or(0)));

        h.usize(struck.len());
        for id in struck {
            id.hash_into(h);
        }
    }
}

/// Whether the hitbox is live on this tick of the swing.
///
/// Half-open: live on `STARTUP`, dead on `STARTUP + ACTIVE`. Written once, here,
/// so the open and close edges cannot drift apart — which is the classic way a
/// window ends up one tick wider than the constant says.
fn is_active(elapsed: u32) -> bool {
    (STARTUP..STARTUP + ACTIVE).contains(&elapsed)
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
/// A press arriving while a swing is in progress is **dropped**, not queued.
/// Buffering it is a real feature and a separate one — it needs an expiry and a
/// rule about which phase accepts it, and guessing those now would bake in a
/// feel nobody has tried.
pub(crate) fn attack(
    state: &mut Attack,
    player_pos: Vec2,
    facing: f32,
    pressed: bool,
    bodies: &Slots,
    pos: &[Vec2],
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
    // RECOVERY the first that can start a new swing, rather than the one after
    // that.
    if let Some(elapsed) = state.elapsed {
        state.elapsed = (elapsed + 1 < SWING).then_some(elapsed + 1);
    }

    if state.elapsed.is_none() && pressed {
        state.elapsed = Some(0);
        state.struck.clear();
        trace.emit(Event::Swung);
    }

    let Some(elapsed) = state.elapsed else { return };

    if elapsed == STARTUP {
        trace.emit(Event::HitboxOpened);
    }

    if is_active(elapsed) {
        strike(state, player_pos, facing, bodies, pos, &mut trace);
    }

    // Symmetric with the open above: `opened` on the first tick the hitbox
    // exists, `closed` on the first tick it does not. Emitting `closed` on the
    // *last* live tick instead would read as a one-tick-shorter window in every
    // trace, which is precisely the confusion these events exist to prevent.
    // Both land inside the swing because RECOVERY is at least one tick.
    if elapsed == STARTUP + ACTIVE {
        trace.emit(Event::HitboxClosed);
    }
}

/// Tests the live hitbox against every body and records what it touches.
fn strike(
    state: &mut Attack,
    player_pos: Vec2,
    facing: f32,
    bodies: &Slots,
    pos: &[Vec2],
    trace: &mut TraceSink<'_>,
) {
    // Yaw 0 faces +Z and positive turns toward +X — the convention
    // `Instance::with_yaw` and `shader.wgsl` share, and the one thing here a
    // sign flip would break silently, by swinging out of the character's back.
    let ahead = Vec2::new(facing.sin(), facing.cos());
    let centre = player_pos + ahead * REACH;
    let contact_distance = HITBOX_RADIUS + ENEMY_RADIUS;

    // **Cheapest test first.** The distance check rejects almost every body in a
    // handful of instructions; the two checks that were above it — a load from
    // a second array, and a linear scan of what this swing has already hit —
    // then run only for the few that are actually in reach. Measured at 16384
    // bodies: 53µs a tick to 4µs.
    for (row, body) in pos.iter().enumerate() {
        if contact::between(centre, *body, contact_distance, row).is_none() {
            continue;
        }

        let id = bodies.ids()[row];
        if state.struck.contains(&id) {
            continue;
        }

        state.struck.push(id);
        trace.emit(Event::Hit { id });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::Trace;

    /// Drives a swing with no bodies to hit, so what is left is the state
    /// machine on its own. The scenarios cover it end to end; these localise a
    /// failure to the phases rather than to the geometry or the schedule.
    fn swing_for(ticks: u32, press_on: &[u32]) -> Vec<(u32, bool, bool)> {
        let mut state = Attack::default();
        let mut trace = Trace::default();
        let mut seen = Vec::new();

        for tick in 0..ticks {
            attack(
                &mut state,
                Vec2::ZERO,
                0.0,
                press_on.contains(&tick),
                &Slots::default(),
                &[],
                trace.sink(u64::from(tick)),
            );
            seen.push((tick, state.is_swinging(), state.hitbox_is_live()));
        }

        seen
    }

    /// The window's edges, stated as the constants state them. Half-open: live
    /// on `STARTUP`, dead on `STARTUP + ACTIVE`.
    #[test]
    fn the_hitbox_is_live_for_exactly_the_active_ticks() {
        assert!(!is_active(STARTUP - 1), "live during startup");
        assert!(is_active(STARTUP), "not live on the first active tick");
        assert!(is_active(STARTUP + ACTIVE - 1), "not live on the last active tick");
        assert!(!is_active(STARTUP + ACTIVE), "still live after the window");

        let live: Vec<u32> = (0..SWING).filter(|&t| is_active(t)).collect();
        assert_eq!(live.len() as u32, ACTIVE, "the window is not ACTIVE ticks wide");
    }

    /// A swing occupies exactly `SWING` ticks and then the player is idle
    /// again — and `elapsed` names the tick that just ran, so this reads the
    /// way the harness reads it.
    #[test]
    fn a_swing_lasts_exactly_its_length() {
        let seen = swing_for(SWING + 2, &[0]);

        for (tick, swinging, _) in &seen[..SWING as usize] {
            assert!(swinging, "idle on tick {tick}, inside the swing");
        }
        assert!(!seen[SWING as usize].1, "still swinging after SWING ticks");
    }

    /// **Hitbox liveness is derived from one number, so it cannot disagree with
    /// the phase.** Checked against the constants rather than against a second
    /// copy of the schedule.
    #[test]
    fn the_hitbox_is_live_only_inside_the_swing() {
        for (tick, swinging, hitbox) in swing_for(SWING + 2, &[0]) {
            let expected = tick < SWING && is_active(tick);
            assert_eq!(hitbox, expected, "hitbox wrong on tick {tick}");
            if hitbox {
                assert!(swinging, "a live hitbox with no swing behind it, tick {tick}");
            }
        }
    }

    /// An idle player with the button down is not attacking. The edge is the
    /// caller's to supply, and this is what says the pass believes it.
    #[test]
    fn no_press_means_no_swing() {
        for (tick, swinging, hitbox) in swing_for(SWING, &[]) {
            assert!(!swinging && !hitbox, "swung with no press, tick {tick}");
        }
    }
}
