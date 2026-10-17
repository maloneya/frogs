/// Validated global contact material, sampled once for a physics step.
/// The simulation validates the response; gameplay supplies accepted settings between ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactMaterial {
    frequency_hz: f32,
    damping_ratio: f32,
    push_speed: f32,
}
impl ContactMaterial {
    /// Lowest supported spring frequency, in hertz.
    pub const MIN_FREQUENCY_HZ: f32 = 1.0;
    /// Lowest supported damping ratio.
    pub const MIN_DAMPING_RATIO: f32 = 0.0;
    /// Highest supported damping ratio.
    pub const MAX_DAMPING_RATIO: f32 = 20.0;
    /// Lowest supported overlap push speed, in metres per second.
    pub const MIN_PUSH_SPEED: f32 = 0.1;
    /// Highest supported overlap push speed, in metres per second.
    pub const MAX_PUSH_SPEED: f32 = 20.0;
    /// Upper bound imposed by the solver substep rate.
    pub const MAX_FREQUENCY_HZ: f32 = crate::solver::MAX_CONTACT_HERTZ;
    /// Validate the complete contact response before replacing accepted settings.
    pub fn new(
        frequency_hz: f32,
        damping_ratio: f32,
        push_speed: f32,
    ) -> Result<Self, &'static str> {
        if !frequency_hz.is_finite()
            || !(Self::MIN_FREQUENCY_HZ..=Self::MAX_FREQUENCY_HZ).contains(&frequency_hz)
            || !damping_ratio.is_finite()
            || !(Self::MIN_DAMPING_RATIO..=Self::MAX_DAMPING_RATIO).contains(&damping_ratio)
            || !push_speed.is_finite()
            || !(Self::MIN_PUSH_SPEED..=Self::MAX_PUSH_SPEED).contains(&push_speed)
        {
            return Err("contact material is outside the supported range");
        }
        Ok(Self {
            frequency_hz,
            damping_ratio,
            push_speed,
        })
    }
    pub(crate) fn hash(self, hash: &mut crate::Fnv) {
        hash.f32(self.frequency_hz);
        hash.f32(self.damping_ratio);
        hash.f32(self.push_speed);
    }
}
impl Default for ContactMaterial {
    fn default() -> Self {
        Self::new(30.0, 10.0, 3.0).expect("default contact material is valid")
    }
}
impl ContactMaterial {
    /// Spring response frequency, in hertz.
    pub fn frequency_hz(self) -> f32 {
        self.frequency_hz
    }
    /// Damping relative to critical damping.
    pub fn damping_ratio(self) -> f32 {
        self.damping_ratio
    }
    /// Maximum overlap separation speed, in metres per second.
    pub fn push_speed(self) -> f32 {
        self.push_speed
    }
}
