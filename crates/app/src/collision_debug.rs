//! Read-only collision visualization. Debug settings never enter gameplay state.

use arpg_core::Report;
use arpg_game::Game;
use arpg_gfx::DebugDisc;
use arpg_sim::{AttackDisc, AttackPhase, CollisionDisc};
use glam::Vec3;

const MAX_DISCS: usize = arpg_sim::MAX_BODIES + 1 + AttackDisc::MAX_SAMPLES;
const _: () = assert!(MAX_DISCS <= arpg_core::MAX_INSTANCES);

#[derive(Default)]
pub(crate) struct CollisionDebug {
    enabled: bool,
    tick: u64,
    snapshots: Vec<CollisionDisc>,
    drawings: Vec<DebugDisc>,
    attack: Vec<AttackDisc>,
    attack_phase: Option<AttackPhase>,
}

impl CollisionDebug {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool, game: &Game) {
        self.enabled = enabled;
        if enabled && self.drawings.capacity() == 0 {
            self.drawings.reserve_exact(MAX_DISCS);
            self.attack.reserve_exact(AttackDisc::MAX_SAMPLES);
            self.snapshots.reserve_exact(arpg_sim::MAX_BODIES + 1);
        }
        self.rebuild(game);
    }

    pub(crate) fn rebuild(&mut self, game: &Game) {
        self.snapshots.clear();
        self.drawings.clear();
        self.attack.clear();
        self.attack_phase = None;
        self.tick = game.tick();
        if !self.enabled {
            return;
        }
        for disc in game.collision_discs() {
            let colour = if disc.id() == game.player_id() {
                Vec3::new(1.0, 0.7, 0.05)
            } else {
                Vec3::new(0.05, 0.8, 1.0)
            };
            self.drawings
                .push(DebugDisc::new(disc.centre(), disc.radius(), colour));
            self.snapshots.push(disc);
        }
        let phase = game.attack_status().phase;
        self.attack_phase = Some(phase);
        let colour = if phase == AttackPhase::Active {
            Vec3::new(1.0, 0.08, 0.02)
        } else {
            Vec3::new(0.3, 0.12, 0.06)
        };
        for disc in game.attack_discs() {
            self.drawings.push(DebugDisc::new(disc.centre(), disc.radius(), colour));
            self.attack.push(disc);
        }
    }

    pub(crate) fn drawings(&self) -> &[DebugDisc] {
        &self.drawings
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.bool("enabled", self.enabled);
        out.int("sample_tick", self.tick);
        out.text("positions", "completed_tick");
        out.int("disc_count", self.drawings.len() as u64);
        out.int("body_disc_count", self.snapshots.len() as u64);
        out.object("attack", |out| {
            out.int("disc_count", self.attack.len() as u64);
            if let Some(phase) = self.attack_phase {
                out.text("phase", &phase.to_string());
            }
            out.object("discs", |out| {
                for disc in &self.attack {
                    out.object(&disc.sample().to_string(), |out| {
                        out.vec3("centre", disc.centre());
                        out.num("radius", disc.radius());
                    });
                }
            });
        });
        out.object("discs", |out| {
            for disc in &self.snapshots {
                out.object(&disc.id().to_string(), |out| {
                    out.vec3("centre", disc.centre());
                    out.num("radius", disc.radius());
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::{Intent, MoveDir};
    use arpg_sim::{Accumulator, Dt, Template};

    #[test]
    fn attack_outlines_follow_committed_samples_and_completed_tick_pose() {
        let mut game = Game::empty();
        game.set_attack_profile(arpg_sim::AttackProfile::Thrust);
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut debug = CollisionDebug::default();
        debug.set_enabled(true, &game);
        assert!(debug.attack.is_empty());

        // Thrust: four startup ticks, four active samples, eight recovery ticks.
        // Move and turn throughout, then select a different next swing mid-attack.
        for elapsed in 0..=16 {
            game.step(dt, Intent::new(MoveDir::new(Vec3::X), elapsed == 0));
            if elapsed == 1 {
                game.set_attack_profile(arpg_sim::AttackProfile::CrowdBreaker);
            }
            let before = game.hash();
            debug.rebuild(&game);
            assert_eq!(game.hash(), before, "observation must not affect gameplay");
            assert_eq!(debug.tick, game.tick());
            let (phase, samples): (_, Vec<usize>) = match elapsed {
                0..=3 => (AttackPhase::Startup, (0..4).collect()),
                4..=7 => (AttackPhase::Active, vec![elapsed - 4]),
                8..=15 => (AttackPhase::Recovery, vec![]),
                _ => (AttackPhase::Idle, vec![]),
            };
            assert_eq!(debug.attack_phase, Some(phase));
            assert_eq!(debug.attack.len(), samples.len());
            assert_eq!(debug.drawings.len(), 1 + samples.len());
            let origin = game.player_pos();
            let (sin, cos) = game.player_facing().sin_cos();
            for (disc, sample) in debug.attack.iter().zip(samples) {
                assert_eq!(disc.sample(), sample);
                let reach = 0.8 + sample as f32 / 3.0;
                let expected = Vec3::new(origin.x + sin * reach, 0.0, origin.z + cos * reach);
                assert!(disc.centre().distance(expected) < 1e-5);
                assert!((disc.radius() - 0.3).abs() < 1e-6);
            }
            let mut report = Report::default();
            debug.report(&mut report);
            let report = report.finish();
            assert!(report.contains(&format!("\"phase\":\"{phase}\"")));
            assert!(report.contains(&format!("\"attack\":{{\"disc_count\":{}", debug.attack.len())));
            if elapsed == 4 {
                debug.set_enabled(false, &game);
                assert!(debug.attack.is_empty());
                assert!(debug.drawings.is_empty());
                debug.set_enabled(true, &game);
                assert_eq!(debug.attack.len(), 1);
                debug.rebuild(&Game::empty());
                assert!(debug.attack.is_empty(), "scene replacement clears attack outlines");
            }
        }
    }

    #[test]
    fn toggle_observes_exact_geometry_without_changing_gameplay() {
        let mut game = Game::empty();
        game.place(glam::Vec2::new(2.0, 0.0), Template::BODY)
            .unwrap();
        game.place(glam::Vec2::new(-2.0, 0.0), Template::BLOCK)
            .unwrap();
        let mut debug = CollisionDebug::default();
        let before = game.hash();
        assert!(debug.drawings().is_empty());
        debug.set_enabled(true, &game);
        assert_eq!(game.hash(), before);
        assert_eq!(debug.drawings().len(), 3);
        assert_eq!(debug.drawings()[0].radius(), 0.3);
        assert_eq!(debug.drawings()[1].radius(), 0.25);
        assert_eq!(debug.drawings()[2].centre(), Vec3::new(-2.0, 0.0, 0.0));
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        game.step(dt, Intent::new(MoveDir::new(Vec3::Z), false));
        debug.rebuild(&game);
        assert_eq!(debug.drawings()[0].centre().z, arpg_sim::WALK_PER_TICK);
        assert_eq!(debug.tick, game.tick());
        debug.rebuild(&Game::empty());
        assert_eq!(
            debug.drawings().len(),
            1,
            "fresh scenes discard old outlines"
        );
        let before = game.hash();
        debug.set_enabled(false, &game);
        assert!(debug.drawings().is_empty());
        assert_eq!(game.hash(), before);
    }
}
