//! Read-only collision visualization. Debug settings never enter gameplay state.

use arpg_core::{LinearRgb, Report};
use arpg_game::Game;
use arpg_gfx::DebugOutline;
use arpg_sim::{AttackDisc, AttackPhase, CollisionDisc};

const MAX_OUTLINES: usize = arpg_sim::MAX_BODIES + 1 + AttackDisc::MAX_SAMPLES;
// Joint boxes replace the discs of the bodies they belong to.
const _: () = assert!(MAX_OUTLINES <= arpg_core::MAX_INSTANCES);

#[derive(Default)]
pub(crate) struct CollisionDebug {
    enabled: bool,
    tick: u64,
    snapshots: Vec<CollisionDisc>,
    drawings: Vec<DebugOutline>,
    attack: Vec<AttackDisc>,
    joints: Vec<arpg_sim::JointPresentation>,
    attack_phase: Option<AttackPhase>,
}

impl CollisionDebug {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool, game: &Game) {
        self.enabled = enabled;
        if enabled && self.drawings.capacity() == 0 {
            self.drawings.reserve_exact(MAX_OUTLINES);
            self.attack.reserve_exact(AttackDisc::MAX_SAMPLES);
            self.joints.reserve_exact(arpg_sim::MAX_JOINTS);
            self.snapshots.reserve_exact(arpg_sim::MAX_BODIES + 1);
        }
        self.rebuild(game);
    }

    pub(crate) fn rebuild(&mut self, game: &Game) {
        self.snapshots.clear();
        self.drawings.clear();
        self.attack.clear();
        self.joints.clear();
        self.attack_phase = None;
        self.tick = game.tick();
        if !self.enabled {
            return;
        }
        for disc in game.collision_discs() {
            let colour = if disc.id() == game.player_id() {
                LinearRgb::new(1.0, 0.7, 0.05)
            } else {
                LinearRgb::new(0.05, 0.8, 1.0)
            };
            self.drawings
                .push(DebugOutline::disc(disc.centre(), disc.radius(), colour));
            self.snapshots.push(disc);
        }
        for joint in game.joint_presentations(arpg_sim::Alpha::ONE) {
            let collider = joint.collider();
            let centre = collider.centre();
            self.drawings.push(DebugOutline::rectangle(
                glam::Vec3::new(centre.x, 0.0, centre.y),
                collider.size(),
                collider.yaw(),
                LinearRgb::new(0.05, 0.8, 1.0),
            ));
            self.joints.push(joint);
        }
        let phase = game.attack_status().phase;
        self.attack_phase = Some(phase);
        let colour = if phase == AttackPhase::Active {
            LinearRgb::new(1.0, 0.08, 0.02)
        } else {
            LinearRgb::new(0.3, 0.12, 0.06)
        };
        for disc in game.attack_discs() {
            self.drawings
                .push(DebugOutline::disc(disc.centre(), disc.radius(), colour));
            self.attack.push(disc);
        }
    }

    pub(crate) fn drawings(&self) -> &[DebugOutline] {
        &self.drawings
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.bool("enabled", self.enabled);
        out.int("sample_tick", self.tick);
        out.text("positions", "completed_tick");
        out.int("outline_count", self.drawings.len() as u64);
        out.int("body_disc_count", self.snapshots.len() as u64);
        out.int("joint_count", self.joints.len() as u64);
        out.object("joints", |out| {
            for joint in &self.joints {
                out.object(&joint.id().to_string(), |out| {
                    let collider = joint.collider();
                    out.vec3("centre", glam::Vec3::new(collider.centre().x, 0.0, collider.centre().y));
                    out.num("width", collider.size().x);
                    out.num("length", collider.size().y);
                    out.num("yaw", collider.yaw());
                    out.text("lock", &format!("{:?}", joint.lock()));
                });
            }
        });
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
    use arpg_sim::{Accumulator, Dt};
    use glam::Vec3;

    #[test]
    fn attack_outlines_follow_committed_samples_and_completed_tick_pose() {
        let mut game = Game::empty();
        game.set_attack_profile(arpg_game::AttackProfile::Slam);
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut debug = CollisionDebug::default();
        debug.set_enabled(true, &game);
        assert!(debug.attack.is_empty());

        // Slam: nine startup ticks, five active samples, sixteen recovery ticks.
        // Move and turn throughout, then select a different next swing mid-attack.
        for elapsed in 0..=30 {
            game.step(
                dt,
                Intent::new(
                    MoveDir::new(Vec3::X),
                    arpg_core::AimDir::new(Vec3::X).unwrap(),
                    elapsed == 0,
                ),
            );
            if elapsed == 1 {
                game.set_attack_profile(arpg_game::AttackProfile::Cleave);
            }
            let before = game.hash();
            debug.rebuild(&game);
            assert_eq!(game.hash(), before, "observation must not affect gameplay");
            assert_eq!(debug.tick, game.tick());
            let (phase, samples): (_, Vec<usize>) = match elapsed {
                0..=8 => (AttackPhase::Startup, (0..5).collect()),
                9..=13 => (AttackPhase::Active, vec![elapsed - 9]),
                14..=29 => (AttackPhase::Recovery, vec![]),
                _ => (AttackPhase::Idle, vec![]),
            };
            assert_eq!(debug.attack_phase, Some(phase));
            assert_eq!(debug.attack.len(), samples.len());
            assert_eq!(debug.drawings.len(), 1 + samples.len());
            let origin = game.player_pos();
            for (disc, sample) in debug.attack.iter().zip(samples) {
                assert_eq!(disc.sample(), sample);
                let expected = Vec3::new(origin.x, 0.0, origin.z);
                assert!(disc.centre().distance(expected) < 1e-5);
                assert!((disc.radius() - (1.4 + 0.7 * sample as f32)).abs() < 1e-6);
            }
            let mut report = Report::default();
            debug.report(&mut report);
            let report = report.finish();
            assert!(report.contains(&format!("\"phase\":\"{phase}\"")));
            assert!(report.contains(&format!(
                "\"attack\":{{\"disc_count\":{}",
                debug.attack.len()
            )));
            if elapsed == 9 {
                debug.set_enabled(false, &game);
                assert!(debug.attack.is_empty());
                assert!(debug.drawings.is_empty());
                debug.set_enabled(true, &game);
                assert_eq!(debug.attack.len(), 1);
                debug.rebuild(&Game::empty());
                assert!(
                    debug.attack.is_empty(),
                    "scene replacement clears attack outlines"
                );
            }
        }
    }

    #[test]
    fn panel_outline_uses_authoritative_rectangle_and_clears_on_scene_eviction() {
        let mut actor = arpg_game::ActorDefinition::prop();
        actor.physical = arpg_sim::Template::BODY;
        let scene = arpg_game::GameScene {
            engine: arpg_game::Scene {
                bodies: vec![arpg_game::Placed {
                    pos: (4.0, 2.0),
                    what: actor,
                }],
                ..Default::default()
            },
            doors: vec![arpg_game::DoorSpec {
                body: 0,
                panel: arpg_game::DoorPanel::new(2.0, 0.2, 0.6, -1.0, 1.0, 12.0).unwrap(),
            }],
            ..Default::default()
        };
        let mut game = Game::from_scene(&scene).unwrap();
        let owner = game.scene_instances().next().unwrap().0;
        let mut debug = CollisionDebug::default();
        let before = game.hash();
        debug.set_enabled(true, &game);
        assert_eq!(game.hash(), before);
        assert_eq!(
            debug.drawings.len(),
            2,
            "one player disc and one joint rectangle"
        );
        assert_eq!(
            debug.snapshots.len(),
            1,
            "a panel must not retain a disc approximation"
        );
        assert_eq!(debug.joints.len(), 1);
        let panel = game
            .joint_presentations(arpg_sim::Alpha::ONE)
            .next()
            .unwrap();
        assert_eq!(
            debug.drawings[1].centre(),
            Vec3::new(panel.collider().centre().x, 0.0, panel.collider().centre().y)
        );
        assert_eq!(
            debug.drawings[1].shape(),
            arpg_gfx::OutlineShape::Rectangle {
                half_extents: panel.collider().size() * 0.5,
                yaw: panel.collider().yaw()
            }
        );
        let mut report = Report::default();
        debug.report(&mut report);
        let report = report.finish();
        assert!(report.contains("\"outline_count\":2"));
        assert!(report.contains("\"joint_count\":1"));
        assert!(game.evict_scene(owner));
        debug.rebuild(&game);
        assert_eq!(debug.drawings.len(), 1);
        assert!(debug.joints.is_empty());
        debug.set_enabled(false, &game);
        assert!(debug.drawings.is_empty());
    }

    #[test]
    fn toggle_observes_exact_geometry_without_changing_gameplay() {
        let mut game = Game::empty();
        game.place(
            glam::Vec2::new(2.0, 0.0),
            arpg_game::ActorDefinition::passive_hostile(),
        )
        .unwrap();
        game.place(
            glam::Vec2::new(-2.0, 0.0),
            arpg_game::ActorDefinition::prop(),
        )
        .unwrap();
        let mut debug = CollisionDebug::default();
        let before = game.hash();
        assert!(debug.drawings().is_empty());
        debug.set_enabled(true, &game);
        assert_eq!(game.hash(), before);
        assert_eq!(debug.drawings().len(), 3);
        assert_eq!(
            debug.drawings()[0].shape(),
            arpg_gfx::OutlineShape::Disc { radius: 0.3 }
        );
        assert_eq!(
            debug.drawings()[1].shape(),
            arpg_gfx::OutlineShape::Disc { radius: 0.25 }
        );
        assert_eq!(debug.drawings()[2].centre(), Vec3::new(-2.0, 0.0, 0.0));
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        game.step(
            dt,
            Intent::new(
                MoveDir::new(Vec3::Z),
                arpg_core::AimDir::new(Vec3::Z).unwrap(),
                false,
            ),
        );
        debug.rebuild(&game);
        // The motor's first-tick change ramps in over four substeps: 5/8 of a dt².
        let travel = arpg_game::Tuning::default().get(arpg_game::TuningField::PlayerAcceleration)
            * arpg_sim::Dt::SECS
            * arpg_sim::Dt::SECS
            * 5.0
            / 8.0;
        assert!((debug.drawings()[0].centre().z - travel).abs() < 1e-6);
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
