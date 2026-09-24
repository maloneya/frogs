//! Single deterministic input schedule shared by live and headless trials.
use arpg_core::{AimDir, Intent, MoveDir};
use glam::Vec3;

/// Validated at the shared harness entry point before any restart or mutation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Spec {
    pub(crate) warmup: u64,
    pub(crate) samples: u64,
    pub(crate) combat: bool,
}

impl Spec {
    pub(crate) fn parse(mut words: std::str::SplitWhitespace<'_>) -> Result<Self, String> {
        let usage = "expected: perf trial <1..3600 warmup ticks> <60..600 sample ticks> idle|combat";
        let warmup = words.next().and_then(|s| s.parse().ok()).ok_or(usage)?;
        let samples = words.next().and_then(|s| s.parse().ok()).ok_or(usage)?;
        let combat = match words.next() {
            Some("idle") => false,
            Some("combat") => true,
            _ => return Err(usage.into()),
        };
        if !(1..=3600).contains(&warmup) || !(60..=600).contains(&samples) || words.next().is_some() {
            return Err(usage.into());
        }
        Ok(Self { warmup, samples, combat })
    }

    pub(crate) fn intent(self, tick: u64) -> Intent {
        if !self.combat || tick < self.warmup {
            return Intent::new(MoveDir::NONE, AimDir::FORWARD, false);
        }
        let tick = tick - self.warmup;
        let direction = [Vec3::X, Vec3::Z, -Vec3::X, -Vec3::Z][(tick / 60 % 4) as usize];
        Intent::new(MoveDir::new(direction), AimDir::new(direction).expect("cardinal aim"), tick.is_multiple_of(30))
    }
}
