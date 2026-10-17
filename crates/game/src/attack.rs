//! Player ability catalog and observations. Physical paths and timing remain in sim.
use arpg_sim::{AttackPhase, AttackShape, RecoveryTicks, ResolvedAttack};
use core::num::NonZeroU8;
use glam::Vec2;
use serde::Deserialize;
/// Authored attacks available to the player today.
///
/// Gameplay input and scenario commands share these identities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum AttackProfile {
    /// A broad, fast arc with a short shove that keeps survivors in follow-up reach.
    #[default]
    Cleave,
    /// An expanding omnidirectional slam with enough impulse to clear a crowd.
    Slam,
    /// A held, piercing beam that follows aim.
    Disintegrate,
}

impl AttackProfile {
    /// The complete authored catalog. Presentation may choose how to lay it out.
    pub const ALL: [Self; 3] = [Self::Cleave, Self::Slam, Self::Disintegrate];

    /// The authored name, shared by state, traces, and selection surfaces.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Cleave => "Cleave",
            Self::Slam => "Slam",
            Self::Disintegrate => "Disintegrate",
        }
    }

    /// Resolves this authored profile into the complete runtime value a swing
    /// captures. Channels return `None`; their capsule and cadence belong to [`crate::Disintegrate`].
    #[must_use]
    pub fn resolve(self, tuning: crate::Tuning) -> Option<ResolvedAttack> {
        let (startup, active, recovery, shape, knockback) = match self {
            Self::Disintegrate => return None,
            // Melee deals two of an ordinary enemy's three health points.
            // Cleave keeps survivors nearby; Slam launches them through the crowd.
            // Crowd-control content uses the same impulse and swept-disc doors
            // as other physical producers. At unit enemy mass and 0.9 retention, free travel is
            // roughly impulse / 6 metres; contacts distribute it through the rank.
            Self::Cleave => (
                6,
                6,
                10,
                shape((-1.8, 1.2, 1.1), (1.8, 1.2, 1.1)),
                tuning.get(crate::TuningField::CleaveImpulse),
            ),
            Self::Slam => (
                9,
                5,
                16,
                shape((0.0, 0.0, 1.4), (0.0, 0.0, 4.2)),
                tuning.get(crate::TuningField::SlamImpulse),
            ),
        };
        Some(
            ResolvedAttack::try_new(
                startup,
                active,
                RecoveryTicks::try_from(recovery).expect("profile recovery is supported"),
                shape,
                knockback,
                NonZeroU8::new(2).expect("melee deals positive damage"),
            )
            .expect("built-in attack profile is valid"),
        )
    }
}

impl core::fmt::Display for AttackProfile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// A read-only snapshot. Changing it cannot change the simulation.
#[derive(Clone, Copy, Debug)]
pub struct AttackStatus {
    /// Shared lifecycle of the committed swing or held channel.
    pub execution: Option<arpg_sim::ActionExecution>,
    /// Current phase.
    pub phase: AttackPhase,
    /// Elapsed ticks in the current swing, or zero when idle.
    pub elapsed: u32,
    /// Authored attack selected for the next swing.
    pub profile: AttackProfile,
    /// Authored attack captured by the swing in flight.
    pub swing_profile: Option<AttackProfile>,
    /// Configuration used by the next swing.
    pub resolved: Option<ResolvedAttack>,
    /// Whether the held beam is active at the completed tick.
    pub channeling: bool,
    /// Completed tick within the active channel damage cadence, absent when idle.
    pub channel_tick: Option<u32>,
    /// Configuration captured by the swing in flight.
    pub swing_resolved: Option<ResolvedAttack>,
    /// Bodies struck by the current or most recent swing.
    pub struck: usize,
}

impl AttackStatus {
    pub(crate) fn report(self, out: &mut arpg_core::Report) {
        let Self {
            execution,
            phase,
            channeling,
            channel_tick,
            elapsed,
            profile,
            swing_profile,
            resolved,
            swing_resolved,
            struck,
        } = self;
        out.object("action_execution", |out| {
            if let Some(execution) = execution {
                execution.report(out);
            }
        });
        out.bool("channeling", channeling);
        out.int("channel_tick", u64::from(channel_tick.unwrap_or(0)));
        out.bool("swinging", phase != AttackPhase::Idle && !channeling);
        out.bool("startup", phase == AttackPhase::Startup);
        out.bool("hitbox", phase == AttackPhase::Active);
        out.bool("recovering", phase == AttackPhase::Recovery);
        out.int("swing_tick", u64::from(elapsed));
        out.int("struck", struck as u64);
        out.text("attack_profile", profile.label());
        out.text(
            "swing_profile",
            swing_profile.map_or("", AttackProfile::label),
        );
        out.object("resolved_attack", |out| {
            if let Some(resolved) = resolved {
                resolved.report(out);
            }
        });
        out.object("swing_resolved_attack", |out| {
            if let Some(resolved) = swing_resolved {
                resolved.report(out);
            }
        });
    }
}

fn shape(start: (f32, f32, f32), end: (f32, f32, f32)) -> AttackShape {
    AttackShape::try_new(
        Vec2::new(start.0, start.1),
        start.2,
        Vec2::new(end.0, end.1),
        end.2,
    )
    .expect("built-in attack shape is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_melee_profiles_resolve_to_distinct_valid_snapshots() {
        let attacks: Vec<_> = AttackProfile::ALL
            .into_iter()
            .filter_map(|profile| profile.resolve(crate::Tuning::default()))
            .collect();
        assert_eq!(attacks.len(), 2);
        assert!(
            AttackProfile::Disintegrate
                .resolve(crate::Tuning::default())
                .is_none()
        );
        assert_ne!(attacks[0], attacks[1]);
        for attack in attacks {
            assert_eq!(attack.damage().get(), 2);
        }
    }
}
