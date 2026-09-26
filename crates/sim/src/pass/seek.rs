//! Chasers choose a sustained gait and ask physics to accelerate toward the player.
//!
//! Choices derive from stable identity and simulation tick, including an individual
//! phase offset. There is no wall-clock RNG or mutable random stream whose result
//! depends on iteration order. Membership, identity and tick already participate
//! in removal, scene eviction, restart and hashing; gait is derived from them.

use glam::Vec2;
use serde::Deserialize;

use super::motion::{DriveSink, stopping_speed};
use crate::members::Members;
use crate::slots::Slots;
use crate::{Dt, EntityId, Event, Fnv, TraceSink, PLAYER_ENEMY_CONTACT_DISTANCE};

/// Ground locomotion selected by a chaser. Actual speed also depends on contacts
/// and acceleration; this describes what the creature is trying to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Gait {
    /// Slow approach.
    Walk,
    /// Ordinary pursuit.
    Run,
    /// Fast burst; its target speed is runtime tuning.
    Sprint,
}

impl Gait {
    /// Stable label shared by reports and scenario diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Walk => "walk",
            Self::Run => "run",
            Self::Sprint => "sprint",
        }
    }

    /// Stateless seeded choice, held for three seconds with staggered boundaries.
    /// Whole identity includes generation: recycling a slot does not repeat its
    /// old occupant's sequence. Dense-row swaps do not affect it.
    pub(crate) fn at(id: EntityId, tick: u64) -> Self {
        Self::sample(id, tick).0
    }

    fn sample(id: EntityId, tick: u64) -> (Self, bool) {
        let mut hash = Fnv::default();
        id.hash_into(&mut hash);
        let seed = hash.finish();
        let period = u64::from(crate::TICK_HZ) * 3;
        let phased_tick = tick.saturating_add(seed % period);
        let epoch = phased_tick / period;
        let gait = Self::choose(seed, epoch);
        let changed = tick > 0 && phased_tick.is_multiple_of(period)
            && gait != Self::choose(seed, epoch - 1);
        (gait, changed)
    }

    fn choose(seed: u64, epoch: u64) -> Self {
        // SplitMix64's output mixer diffuses neighbouring epochs and identities.
        let mut value = seed.wrapping_add(epoch.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        match value % 3 {
            0 => Self::Walk,
            1 => Self::Run,
            _ => Self::Sprint,
        }
    }
}

/// Reads post-player-walk positions and grants no structural capabilities.
/// The motor changes velocity now; motion::integrate moves it later this tick,
/// before contacts exchange the resulting momentum. Near arrival, braking keeps
/// an unimpeded chaser from continually walking into the player.
#[expect(clippy::too_many_arguments, reason = "separate read slices, motor capability, tuning and tick inputs keep pass access explicit")]
pub(crate) fn seek(
    seekers: &Members,
    bodies: &Slots,
    pos: &[Vec2],
    mut drive: DriveSink<'_>,
    tuning: crate::Tuning,
    tick: u64,
    dt: Dt,
    mut trace: TraceSink<'_>,
) {
    let target = pos[0];
    let mut changed_counts = [0; 3];
    for &id in seekers.ids() {
        let Some(row) = bodies.index(id) else { continue };
        let to_target = target - pos[row];
        let distance = to_target.length();
        let remaining = (distance - PLAYER_ENEMY_CONTACT_DISTANCE).max(0.0);
        let (gait, changed) = Gait::sample(id, tick);
        if changed {
            changed_counts[match gait { Gait::Walk => 0, Gait::Run => 1, Gait::Sprint => 2 }] += 1;
        }
        let speed = tuning.gait_speed(gait).min(stopping_speed(remaining, dt, tuning.get(crate::TuningField::MonsterAcceleration)));
        let desired = if remaining > 0.0 { to_target / distance * speed } else { Vec2::ZERO };
        drive.toward(id, desired, dt, tuning.get(crate::TuningField::MonsterAcceleration));
    }
    let [walking, running, sprinting] = changed_counts;
    if walking + running + sprinting > 0 {
        trace.emit(Event::GaitsChanged { walking, running, sprinting });
    }
}
