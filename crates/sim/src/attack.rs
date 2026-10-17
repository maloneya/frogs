//! Validated resolved values and physical paths consumed by gameplay attack producers.

use glam::Vec2;
use core::num::NonZeroU8;
use serde::Deserialize;

use crate::swing::Swing;

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
        if !crate::valid_position(start) || !crate::valid_position(end) {
            return Err(AttackResolveError::InvalidShapePosition);
        }
        if (start == Vec2::ZERO) != (end == Vec2::ZERO) {
            return Err(AttackResolveError::DirectionlessShape);
        }
        if !start_radius.is_finite()
            || !end_radius.is_finite()
            || start_radius <= 0.0
            || end_radius <= 0.0
            || start_radius > crate::MAX_POSITION_COMPONENT
            || end_radius > crate::MAX_POSITION_COMPONENT
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
    /// A path endpoint lies outside the finite geometric arithmetic domain.
    InvalidShapePosition,
    /// Only one path endpoint is at the producer origin, leaving its arc undefined.
    DirectionlessShape,
    /// A hitbox radius is non-positive or outside the finite geometric domain.
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
            Self::InvalidShapePosition => "attack path must be within the finite geometric domain",
            Self::DirectionlessShape => "attack path must be centred throughout or have two directed endpoints",
            Self::InvalidRadius => "attack radii must be positive and within the finite geometric domain",
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
    damage: NonZeroU8,
}

impl ResolvedAttack {
    /// Maximum supported active duration and committed sample count.
    pub const MAX_ACTIVE_TICKS: u32 = 120;
    const MIN_TICKS: u32 = 1;
    const MAX_TICKS: u32 = 120;

    /// Validates one complete resolved attack in a single transaction.
    pub fn try_new(
        startup: u32,
        active: u32,
        recovery: RecoveryTicks,
        shape: AttackShape,
        knockback: f32,
        damage: NonZeroU8,
    ) -> Result<Self, AttackResolveError> {
        if !(Self::MIN_TICKS..=Self::MAX_TICKS).contains(&startup)
            || !(Self::MIN_TICKS..=Self::MAX_ACTIVE_TICKS).contains(&active)
        {
            return Err(AttackResolveError::InvalidTiming);
        }
        if !knockback.is_finite() || knockback <= 0.0 || !(knockback * knockback).is_finite() {
            return Err(AttackResolveError::InvalidKnockback);
        }
        let resolved = Self { startup, active, recovery, shape, knockback, damage };
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

    /// Local X/Y: right of facing, then ahead of the producer.
    #[must_use]
    pub fn start(self) -> Vec2 {
        self.shape.start
    }

    /// How big the hitbox is where the swing starts.
    #[must_use]
    pub fn start_radius(self) -> f32 {
        self.shape.start_radius
    }

    /// Local X/Y: right of facing, then ahead of the producer.
    #[must_use]
    pub fn end(self) -> Vec2 {
        self.shape.end
    }

    /// How big the hitbox is where the swing ends.
    #[must_use]
    pub fn end_radius(self) -> f32 {
        self.shape.end_radius
    }

    /// Impulse magnitude; the producer chooses its direction.
    #[must_use]
    pub fn knockback(self) -> f32 {
        self.knockback
    }

    /// Positive health points removed once per target per swing.
    #[must_use]
    pub fn damage(self) -> NonZeroU8 {
        self.damage
    }

    /// Feed the complete admitted value into deterministic replay state.
    pub fn hash_into(self, h: &mut crate::hash::Fnv) {
        let Self { startup, active, recovery, shape, knockback, damage } = self;
        h.u64(u64::from(damage.get()));
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
        let Self { startup, active, recovery, shape, knockback, damage } = self;
        out.int("damage", u64::from(damage.get()));
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

    /// Generate committed physical samples from this fully validated value.
    /// Refuses storage smaller than the active window without partial output.
    pub fn hitbox<const N: usize>(self) -> Option<crate::Hitbox<N>> {
        if self.active as usize > N { return None; }
        Some(Swing::new(self.shape.start, self.shape.end, (self.shape.start_radius, self.shape.end_radius))
            .generate(self.active as usize))
    }

    /// Consecutive samples must overlap enough to cover every supported body.
    fn path_covers_every_body(self) -> bool {
        let hitbox = self.hitbox::<{ Self::MAX_ACTIVE_TICKS as usize }>()
            .expect("validated active window fits committed sample storage");
        hitbox.discs().windows(2).all(|pair| {
            let (from, from_radius) = pair[0].place(Vec2::ZERO, 0.0);
            let (to, to_radius) = pair[1].place(Vec2::ZERO, 0.0);
            from.distance(to) <= from_radius + to_radius + 2.0 * crate::ENEMY_RADIUS
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    fn shape(start: (f32, f32, f32), end: (f32, f32, f32)) -> AttackShape {
        AttackShape::try_new(Vec2::new(start.0, start.1), start.2, Vec2::new(end.0, end.1), end.2).unwrap()
    }
    fn resolved() -> ResolvedAttack {
        ResolvedAttack::try_new(2, 3, RecoveryTicks::try_from(4).unwrap(), shape((-1.0, 1.0, 0.5), (1.0, 1.0, 0.5)), 2.0, NonZeroU8::new(2).unwrap()).unwrap()
    }

    #[test]
    fn recovery_bounds_cannot_be_bypassed() {
        assert!(RecoveryTicks::try_from(0).is_err());
        assert!(RecoveryTicks::try_from(RecoveryTicks::MIN).is_ok());
        assert!(RecoveryTicks::try_from(RecoveryTicks::MAX).is_ok());
        assert!(RecoveryTicks::try_from(RecoveryTicks::MAX + 1).is_err());
        assert!(RecoveryTicks::try_from(u32::MAX).is_err());
    }

    #[test]
    fn damage_is_captured_hashed_and_reported() {
        let attack = resolved();
        let weaker = ResolvedAttack::try_new(attack.startup, attack.active, attack.recovery,
            attack.shape, attack.knockback, NonZeroU8::MIN).unwrap();
        let mut a = crate::hash::Fnv::default();
        let mut b = crate::hash::Fnv::default();
        attack.hash_into(&mut a);
        weaker.hash_into(&mut b);
        assert_ne!(a.finish(), b.finish());
        let mut report = arpg_core::Report::default();
        attack.report(&mut report);
        assert!(report.finish().contains("\"damage\":2"));
    }

    #[test]
    fn geometry_admission_prevents_nonfinite_samples_and_sampling_refuses_small_storage() {
        for invalid in [f32::NAN, f32::INFINITY, f32::MAX] {
            assert_eq!(AttackShape::try_new(Vec2::splat(invalid), 1.0, Vec2::Y, 1.0), Err(AttackResolveError::InvalidShapePosition));
            assert_eq!(AttackShape::try_new(Vec2::Y, invalid, Vec2::Y, 1.0), Err(AttackResolveError::InvalidRadius));
        }
        let extreme = AttackShape::try_new(Vec2::splat(crate::MAX_POSITION_COMPONENT), crate::MAX_POSITION_COMPONENT,
            Vec2::splat(crate::MAX_POSITION_COMPONENT), crate::MAX_POSITION_COMPONENT).unwrap();
        let attack = ResolvedAttack::try_new(1, 1, RecoveryTicks::try_from(1).unwrap(), extreme, 1.0, NonZeroU8::MIN).unwrap();
        let samples = attack.hitbox::<1>().unwrap();
        let (centre, radius) = samples.at(0).place(Vec2::splat(-crate::MAX_POSITION_COMPONENT), 0.75);
        assert!(centre.is_finite() && radius.is_finite());
        assert!(resolved().hitbox::<2>().is_none());
        assert_eq!(resolved().hitbox::<3>().unwrap().discs().len(), 3);
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
            ResolvedAttack::try_new(1, 2, RecoveryTicks::try_from(10).unwrap(), narrow, 1.0, NonZeroU8::MIN),
            Err(AttackResolveError::UncoveredPath)
        );
        assert!(AttackShape::try_new(Vec2::ZERO, 1.4, Vec2::ZERO, 4.2).is_ok());
        assert_eq!(
            AttackShape::try_new(Vec2::Y, 0.5, Vec2::ZERO, 0.5),
            Err(AttackResolveError::DirectionlessShape)
        );
        assert_eq!(
            AttackShape::try_new(Vec2::ZERO, 0.5, Vec2::Y, 0.5),
            Err(AttackResolveError::DirectionlessShape)
        );
        assert_eq!(
            ResolvedAttack::try_new(
                0,
                1,
                RecoveryTicks::try_from(10).unwrap(),
                shape((0.0, 1.0, 0.5), (0.0, 1.0, 0.5)),
                1.0,
                NonZeroU8::MIN,
            ),
            Err(AttackResolveError::InvalidTiming)
        );
    }
}
