//! Authored attack content and the validated runtime values swings consume.

use glam::Vec2;
use serde::Deserialize;

use crate::swing::Swing;

/// Authored attacks available to the player today.
///
/// These are game content, not debug-menu rows. The menu and scenario runner
/// select the same identity that a future skill or weapon will select; neither
/// authors an attack by replaying a sequence of UI increments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum AttackProfile {
    /// The original short, stationary hit in front of the player.
    #[default]
    Basic,
    /// A quick narrow line away from the player.
    Thrust,
    /// A medium-speed side-to-side arc.
    Sweep,
    /// A slower, broader arc with more knockback.
    HeavySweep,
}

impl AttackProfile {
    /// The complete authored catalog. Presentation may choose how to lay it out.
    pub const ALL: [Self; 4] = [Self::Basic, Self::Thrust, Self::Sweep, Self::HeavySweep];

    /// The authored name, shared by state, traces, and selection surfaces.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Thrust => "Thrust",
            Self::Sweep => "Sweep",
            Self::HeavySweep => "Heavy sweep",
        }
    }

    /// Resolves this authored profile into the complete runtime value a swing
    /// captures. A future skill/weapon/stat resolver produces this same type.
    #[must_use]
    pub fn resolve(self) -> ResolvedAttack {
        let (startup, active, recovery, shape, knockback) = match self {
            Self::Basic => (6, 4, 10, shape((0.0, 1.1, 0.6), (0.0, 1.1, 0.6)), 6.0),
            Self::Thrust => (4, 4, 8, shape((0.0, 0.8, 0.3), (0.0, 1.8, 0.3)), 4.0),
            Self::Sweep => (6, 5, 10, shape((-0.8, 0.8, 0.35), (0.8, 0.8, 0.35)), 6.0),
            Self::HeavySweep => (12, 7, 18, shape((-1.0, 0.8, 0.5), (1.0, 0.8, 0.6)), 10.0),
        };
        ResolvedAttack::try_new(
            startup,
            active,
            RecoveryTicks::try_from(recovery).expect("profile recovery is supported"),
            shape,
            knockback,
        )
        .expect("built-in attack profile is valid")
    }
}

impl core::fmt::Display for AttackProfile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// The path and size of a hitbox before timing or force are attached.
///
/// Construction is atomic: a resolver may calculate all four values from
/// skills, equipment and stats, then validate the finished result without
/// becoming dependent on the order in which individual fields were edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackShape {
    start: Vec2,
    start_radius: f32,
    end: Vec2,
    end_radius: f32,
}

impl AttackShape {
    /// Builds one complete local-space path.
    pub fn try_new(
        start: Vec2,
        start_radius: f32,
        end: Vec2,
        end_radius: f32,
    ) -> Result<Self, AttackResolveError> {
        if !start.is_finite() || !end.is_finite() {
            return Err(AttackResolveError::NonFiniteShape);
        }
        if start == Vec2::ZERO || end == Vec2::ZERO {
            return Err(AttackResolveError::DirectionlessShape);
        }
        if !start_radius.is_finite()
            || !end_radius.is_finite()
            || start_radius <= 0.0
            || end_radius <= 0.0
        {
            return Err(AttackResolveError::InvalidRadius);
        }
        Ok(Self { start, start_radius, end, end_radius })
    }
}

/// Why a fully resolved attack could not enter simulation state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackResolveError {
    /// Startup or active duration lies outside the supported tick range.
    InvalidTiming,
    /// A path endpoint contains NaN or infinity.
    NonFiniteShape,
    /// A path endpoint is the player origin and therefore has no direction.
    DirectionlessShape,
    /// A hitbox radius is non-finite or non-positive.
    InvalidRadius,
    /// Knockback cannot produce a finite impulse.
    InvalidKnockback,
    /// Consecutive per-tick hitboxes leave room to skip a body.
    UncoveredPath,
}

impl core::fmt::Display for AttackResolveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidTiming => "attack timing is outside the supported range",
            Self::NonFiniteShape => "attack path must be finite",
            Self::DirectionlessShape => "attack path endpoints need a direction",
            Self::InvalidRadius => "attack radii must be finite and positive",
            Self::InvalidKnockback => "attack knockback must have a finite magnitude",
            Self::UncoveredPath => "attack path can skip a body between ticks",
        })
    }
}

/// The complete configuration captured when a swing begins.
///
/// This is the resolved value, not the authored source of its numbers. A preset
/// can resolve directly to it today; later a skill, weapon, stats and buffs can
/// resolve together and enter through [`ResolvedAttack::try_new`]. The swing only
/// needs the result and remains ignorant of why each number has that value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedAttack {
    startup: u32,
    active: u32,
    recovery: RecoveryTicks,
    shape: AttackShape,
    knockback: f32,
}

impl ResolvedAttack {
    pub(crate) const MAX_ACTIVE_TICKS: u32 = 120;
    const MIN_TICKS: u32 = 1;
    const MAX_TICKS: u32 = 120;

    /// Validates one complete resolved attack in a single transaction.
    pub fn try_new(
        startup: u32,
        active: u32,
        recovery: RecoveryTicks,
        shape: AttackShape,
        knockback: f32,
    ) -> Result<Self, AttackResolveError> {
        if !(Self::MIN_TICKS..=Self::MAX_TICKS).contains(&startup)
            || !(Self::MIN_TICKS..=Self::MAX_ACTIVE_TICKS).contains(&active)
        {
            return Err(AttackResolveError::InvalidTiming);
        }
        if !knockback.is_finite() || knockback <= 0.0 || !(knockback * knockback).is_finite() {
            return Err(AttackResolveError::InvalidKnockback);
        }
        let resolved = Self { startup, active, recovery, shape, knockback };
        if !resolved.path_covers_every_body() {
            return Err(AttackResolveError::UncoveredPath);
        }
        Ok(resolved)
    }

    /// Ticks of wind-up before the hitbox opens.
    #[must_use]
    pub fn startup(self) -> u32 {
        self.startup
    }

    /// Ticks the hitbox stays open.
    #[must_use]
    pub fn active(self) -> u32 {
        self.active
    }

    /// Ticks after the hitbox shuts before another swing may start.
    #[must_use]
    pub fn recovery(self) -> RecoveryTicks {
        self.recovery
    }

    /// Local X/Y: right of facing, then ahead of the player.
    #[must_use]
    pub fn start(self) -> Vec2 {
        self.shape.start
    }

    /// How big the hitbox is where the swing starts.
    #[must_use]
    pub fn start_radius(self) -> f32 {
        self.shape.start_radius
    }

    /// Local X/Y: right of facing, then ahead of the player.
    #[must_use]
    pub fn end(self) -> Vec2 {
        self.shape.end
    }

    /// How big the hitbox is where the swing ends.
    #[must_use]
    pub fn end_radius(self) -> f32 {
        self.shape.end_radius
    }

    /// Impulse magnitude applied along the facing to a struck body.
    #[must_use]
    pub fn knockback(self) -> f32 {
        self.knockback
    }

    /// A semantic runtime modifier already used by the recovery playtest.
    /// It preserves the profile identity while changing the resolved value.
    #[must_use]
    pub(crate) fn with_recovery(mut self, recovery: RecoveryTicks) -> Self {
        self.recovery = recovery;
        self
    }

    pub(crate) fn hash(self, h: &mut crate::hash::Fnv) {
        let Self { startup, active, recovery, shape, knockback } = self;
        h.u64(u64::from(startup));
        h.u64(u64::from(active));
        h.u64(u64::from(recovery.get()));
        for value in [
            shape.start.x,
            shape.start.y,
            shape.start_radius,
            shape.end.x,
            shape.end.y,
            shape.end_radius,
            knockback,
        ] {
            h.f32(value);
        }
    }

    /// Describes every field without asking a control surface to restate them.
    pub fn report(self, out: &mut arpg_core::Report) {
        let Self { startup, active, recovery, shape, knockback } = self;
        out.int("startup_ticks", u64::from(startup));
        out.int("active_ticks", u64::from(active));
        out.int("recovery_ticks", u64::from(recovery.get()));
        out.num("start_right", shape.start.x);
        out.num("start_ahead", shape.start.y);
        out.num("start_radius", shape.start_radius);
        out.num("end_right", shape.end.x);
        out.num("end_ahead", shape.end.y);
        out.num("end_radius", shape.end_radius);
        out.num("knockback", knockback);
    }

    /// Consecutive per-tick discs must overlap enough that an enemy centre
    /// cannot sit between them without touching either. Runtime resolution cannot
    /// use the old const assertion, so the validating edit door owns the rule.
    fn path_covers_every_body(self) -> bool {
        let path = Swing::new(
            self.shape.start,
            self.shape.end,
            (self.shape.start_radius, self.shape.end_radius),
        );
        let hitbox = path.generate::<{ Self::MAX_ACTIVE_TICKS as usize }>(self.active as usize);
        hitbox.discs().windows(2).all(|pair| {
            let (from, from_radius) = pair[0].place(Vec2::ZERO, 0.0);
            let (to, to_radius) = pair[1].place(Vec2::ZERO, 0.0);
            from.distance(to) <= from_radius + to_radius + 2.0 * crate::ENEMY_RADIUS
        })
    }
}

impl Default for ResolvedAttack {
    fn default() -> Self {
        AttackProfile::default().resolve()
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
    /// Authored attack selected for the next swing.
    pub profile: AttackProfile,
    /// Authored attack captured by the swing in flight.
    pub swing_profile: Option<AttackProfile>,
    /// Configuration used by the next swing.
    pub resolved: ResolvedAttack,
    /// Configuration captured by the swing in flight.
    pub swing_resolved: Option<ResolvedAttack>,
    /// Recovery used by the next swing.
    pub recovery: RecoveryTicks,
    /// Recovery committed to by the swing already in flight.
    pub swing_recovery: Option<RecoveryTicks>,
    /// Bodies struck by the current or most recent swing.
    pub struck: usize,
}

impl AttackStatus {
    pub(crate) fn report(self, out: &mut arpg_core::Report) {
        let Self {
            phase,
            elapsed,
            profile,
            swing_profile,
            resolved,
            swing_resolved,
            recovery,
            swing_recovery,
            struck,
        } = self;
        out.bool("swinging", phase != AttackPhase::Idle);
        out.bool("startup", phase == AttackPhase::Startup);
        out.bool("hitbox", phase == AttackPhase::Active);
        out.bool("recovering", phase == AttackPhase::Recovery);
        out.int("swing_tick", u64::from(elapsed));
        out.int("recovery_ticks", u64::from(recovery.get()));
        out.int("swing_recovery_ticks", u64::from(swing_recovery.map_or(0, RecoveryTicks::get)));
        out.int("struck", struck as u64);
        out.text("attack_profile", profile.label());
        out.text("swing_profile", swing_profile.map_or("", AttackProfile::label));
        out.object("resolved_attack", |out| resolved.report(out));
        out.object("swing_resolved_attack", |out| {
            if let Some(resolved) = swing_resolved {
                resolved.report(out);
            }
        });
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

    #[test]
    fn every_authored_profile_resolves_to_a_distinct_valid_attack() {
        let attacks: Vec<_> = AttackProfile::ALL.into_iter().map(AttackProfile::resolve).collect();
        assert_eq!(attacks.len(), AttackProfile::ALL.len());
        for (index, resolved) in attacks.iter().enumerate() {
            assert!(attacks[index + 1..].iter().all(|other| other != resolved));
        }
    }

    #[test]
    fn complete_runtime_values_are_validated_atomically() {
        let narrow = AttackShape::try_new(
            Vec2::new(-4.0, 0.5),
            0.1,
            Vec2::new(4.0, 0.5),
            0.1,
        )
        .unwrap();
        assert_eq!(
            ResolvedAttack::try_new(1, 2, RecoveryTicks::default(), narrow, 1.0),
            Err(AttackResolveError::UncoveredPath)
        );
        assert_eq!(
            AttackShape::try_new(Vec2::ZERO, 0.5, Vec2::Y, 0.5),
            Err(AttackResolveError::DirectionlessShape)
        );
        assert_eq!(
            ResolvedAttack::try_new(
                0,
                1,
                RecoveryTicks::default(),
                shape((0.0, 1.0, 0.5), (0.0, 1.0, 0.5)),
                1.0,
            ),
            Err(AttackResolveError::InvalidTiming)
        );
    }
}
