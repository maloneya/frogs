//! Public attack observations and validated tuning, shared by every control surface.

use serde::Deserialize;

/// Recovery duration in simulation ticks. Construction and deserialization share
/// one validator, so UI, harness and scenarios cannot bypass the timing bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u32")]
pub struct RecoveryTicks(u32);

impl RecoveryTicks {
    /// A swing must spend at least one tick recovering.
    pub const MIN: u32 = 1;
    /// Two seconds at the current tick rate: the supported tuning range.
    /// Bounding this also keeps elapsed-time arithmetic from overflowing.
    pub const MAX: u32 = 2 * crate::TICK_HZ;

    /// Duration in simulation ticks.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl Default for RecoveryTicks {
    fn default() -> Self {
        Self::try_from(10).expect("default recovery lies within its supported range")
    }
}

impl TryFrom<u32> for RecoveryTicks {
    type Error = &'static str;

    fn try_from(ticks: u32) -> Result<Self, Self::Error> {
        if (Self::MIN..=Self::MAX).contains(&ticks) {
            Ok(Self(ticks))
        } else {
            Err("recovery ticks outside the supported range")
        }
    }
}

/// The observed phase of an attack, derived from its elapsed ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum AttackPhase {
    /// Ready to accept an attack.
    Idle,
    /// Winding up; the hitbox does not exist yet.
    Startup,
    /// The hitbox can strike bodies.
    Active,
    /// The hitbox is gone; the next attack is still locked out.
    Recovery,
}

impl core::fmt::Display for AttackPhase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Idle => "Idle",
            Self::Startup => "Startup",
            Self::Active => "Active",
            Self::Recovery => "Recovery",
        })
    }
}

/// A read-only snapshot. Changing it cannot change the simulation.
#[derive(Clone, Copy, Debug)]
pub struct AttackStatus {
    /// Current phase.
    pub phase: AttackPhase,
    /// Elapsed ticks in the current swing, or zero when idle.
    pub elapsed: u32,
    /// Recovery used by the next swing.
    pub recovery: RecoveryTicks,
    /// Recovery committed to by the swing already in flight.
    pub swing_recovery: Option<RecoveryTicks>,
    /// Bodies struck by the current or most recent swing.
    pub struck: usize,
}

impl AttackStatus {
    pub(crate) fn report(self, out: &mut arpg_core::Report) {
        let Self { phase, elapsed, recovery, swing_recovery, struck } = self;
        out.bool("swinging", phase != AttackPhase::Idle);
        out.bool("startup", phase == AttackPhase::Startup);
        out.bool("hitbox", phase == AttackPhase::Active);
        out.bool("recovering", phase == AttackPhase::Recovery);
        out.int("swing_tick", u64::from(elapsed));
        out.int("recovery_ticks", u64::from(recovery.get()));
        out.int("swing_recovery_ticks", u64::from(swing_recovery.map_or(0, RecoveryTicks::get)));
        out.int("struck", struck as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_bounds_cannot_be_bypassed() {
        assert!(RecoveryTicks::try_from(0).is_err());
        assert!(RecoveryTicks::try_from(RecoveryTicks::MIN).is_ok());
        assert!(RecoveryTicks::try_from(RecoveryTicks::MAX).is_ok());
        assert!(RecoveryTicks::try_from(RecoveryTicks::MAX + 1).is_err());
        assert!(RecoveryTicks::try_from(u32::MAX).is_err());
        assert!(RecoveryTicks::try_from(RecoveryTicks::default().get()).is_ok());
    }
}
