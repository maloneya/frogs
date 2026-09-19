//! Bounded, tick-scheduled performance driving through the ordinary Game step.
//! Only the harness can start a trial. No timings decide simulation inputs.

use arpg_core::{AimDir, Intent, MoveDir, Report};
use glam::Vec3;
use std::sync::mpsc::Sender;

/// Validated at the shared harness entry point before any restart or mutation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Spec {
    warmup: u64,
    samples: u64,
    combat: bool,
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

    fn intent(self, tick: u64) -> Intent {
        if !self.combat || tick < self.warmup {
            return Intent::new(MoveDir::NONE, AimDir::FORWARD, false);
        }
        let tick = tick - self.warmup;
        let direction = [Vec3::X, Vec3::Z, -Vec3::X, -Vec3::Z][(tick / 60 % 4) as usize];
        Intent::new(MoveDir::new(direction), AimDir::new(direction).expect("cardinal aim"), tick.is_multiple_of(30))
    }
}

pub(crate) struct Trial {
    spec: Spec,
    reply: Sender<String>,
    measuring: bool,
    initial_enemies: usize,
    start_enemies: usize,
    population_sum: u64,
    invalid: Option<&'static str>,
}

impl Trial {
    pub(crate) fn new(spec: Spec, reply: Sender<String>, initial_enemies: usize) -> Self {
        Self { spec, reply, initial_enemies, measuring: false, start_enemies: 0, population_sum: 0, invalid: None }
    }

    /// Start measurement on a fresh frame, after the exact warm-up endpoint.
    pub(crate) fn begin_frame(&mut self, tick: u64, enemies: usize) -> bool {
        if !self.measuring && tick == self.spec.warmup {
            self.measuring = true;
            self.start_enemies = enemies;
            return true;
        }
        false
    }

    pub(crate) fn finished(&self, tick: u64) -> bool { tick == self.spec.warmup + self.spec.samples }

    /// Stop at either boundary, even when a frame bought multiple ticks. This
    /// discards only boundary-frame excess; interior frames retain normal catch-up.
    pub(crate) fn boundary(&self, tick: u64) -> bool {
        tick == if self.measuring { self.spec.warmup + self.spec.samples } else { self.spec.warmup }
    }

    pub(crate) fn intent(&self, tick: u64) -> Intent { self.spec.intent(tick) }

    pub(crate) fn stepped(&mut self, enemies: usize) {
        if self.measuring { self.population_sum += enemies as u64; }
    }

    pub(crate) fn invalidate(&mut self, reason: &'static str) { self.invalid.get_or_insert(reason); }

    pub(crate) fn report(&self, out: &mut Report) {
        out.int("schedule_version", 1);
        out.int("warmup_ticks", self.spec.warmup);
        out.int("sample_ticks", self.spec.samples);
        out.text("input", if self.spec.combat { "combat" } else { "idle" });
        out.int("initial_enemies", self.initial_enemies as u64);
        out.int("start_enemies", self.start_enemies as u64);
        out.int("population_sum", self.population_sum);
        out.text("invalid", self.invalid.unwrap_or(""));
    }

    pub(crate) fn finish(self, response: String) { let _ = self.reply.send(response); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_game::{Game, GameScene};
    use arpg_sim::{Accumulator, Dt};

    #[test]
    fn rejects_unbounded_and_ambiguous_trials() {
        for text in ["0 60 idle", "1 0 idle", "3601 60 idle", "1 601 idle", "1 60 fight", "1 60 idle extra", "-1 60 idle"] {
            assert!(Spec::parse(text.split_whitespace()).is_err(), "{text}");
        }
    }

    #[test]
    fn catch_up_frames_do_not_change_inputs_or_trial_endpoints() {
        // Prediction: only sample ticks 0 and 30 ask for attacks; each of the
        // first 60 sample ticks walks east. Frame grouping cannot change Game.
        let spec = Spec::parse("5 60 combat".split_whitespace()).unwrap();
        assert!(!spec.intent(4).attack());
        assert!(spec.intent(5).attack());
        assert!(!spec.intent(6).attack());
        assert!(spec.intent(35).attack());
        assert_eq!(spec.intent(64).move_dir(), MoveDir::new(Vec3::X));
        let drive = |frame_ticks: usize| {
            let mut game = Game::from_scene(&GameScene::default()).unwrap();
            let (reply, _rx) = std::sync::mpsc::channel();
            let mut trial = Trial::new(spec, reply, game.enemy_count());
            let mut samples = 0;
            while !trial.finished(game.tick()) {
                if trial.begin_frame(game.tick(), game.enemy_count()) { samples += 1; }
                for _ in 0..frame_ticks {
                    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
                    game.step(dt, trial.intent(game.tick()));
                    trial.stepped(game.enemy_count());
                    if trial.boundary(game.tick()) { break; }
                }
            }
            assert_eq!(samples, 1);
            assert_eq!(game.tick(), 65);
            assert!((game.player_pos().x - 9.0).abs() < 0.0001, "60 eastward ticks at 9 metres/second");
            (game.hash(), game.trace().render(), trial.population_sum)
        };
        assert_eq!(drive(1), drive(6));
    }
}
