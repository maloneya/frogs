//! Read-only timing observers cannot change the Game schedule or its outcomes.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{ActorDefinition, BodyGrid, Game, Scene};
use arpg_sim::{Accumulator, Dt};
use glam::Vec3;

#[test]
fn crowd_observations_follow_the_same_game_schedule_without_changing_replay() {
    #[derive(Default)]
    struct Observer {
        active: bool, starts: usize, finishes: usize,
        candidates: u64, possible_pairs: u64,
        work: arpg_sim::CrowdWork,
        rows: u64, phases: usize, substeps: u64,
    }
    impl arpg_sim::StepObserver for Observer {
        const PHYSICS_DETAILS: bool = true;
        fn physics_phase(&mut self, _: arpg_sim::PhysicsPhase) {
            assert!(self.active);
            self.phases += 1;
        }
        fn physics_count(&mut self, counter: arpg_sim::PhysicsCounter, amount: u64) {
            assert!(self.active);
            match counter {
                arpg_sim::PhysicsCounter::Substeps => self.substeps += amount,
                arpg_sim::PhysicsCounter::BodyRows => self.rows += amount,
                _ => {}
            }
        }
        fn crowd_started(&mut self) {
            assert!(!self.active);
            self.active = true;
            self.starts += 1;
        }
        fn crowd_finished(&mut self, work: arpg_sim::CrowdWork) {
            assert!(self.active);
            self.active = false;
            self.finishes += 1;
            self.candidates += work.candidates();
            self.possible_pairs += work.possible_pairs();
            self.work = work;
        }
    }
    let scene = arpg_game::GameScene {
        engine: Scene {
            grids: vec![BodyGrid {
                origin: (-3.0, -3.0), columns: 10, rows: 10, spacing: 0.6,
                what: ActorDefinition::hostile(),
            }], ..Default::default()
        }, ..Default::default()
    };
    let mut observed = Game::from_scene(&scene).unwrap();
    let mut ordinary = Game::from_scene(&scene).unwrap();
    let mut observer = Observer::default();
    for tick in 0..90 {
        observer.candidates = 0;
        observer.possible_pairs = 0;
        // Collision geometry includes moving corpses as well as living characters.
        let before = observed.collision_discs().count() as u64 - 1;
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let intent = Intent::new(MoveDir::new(Vec3::X), AimDir::FORWARD, tick % 25 == 0);
        observed.step_observed(dt, intent, &mut observer);
        ordinary.step(dt, intent);
        let per_sweep = before * before.saturating_sub(1) / 2;
        assert!(observer.possible_pairs >= per_sweep);
        if per_sweep > 0 {
            assert_eq!(observer.possible_pairs % per_sweep, 0);
        }
        assert!(observer.candidates <= observer.possible_pairs);
        assert_eq!(observer.work.contacts(), observed.crowd_contacts());
        assert_eq!(observed.hash(), ordinary.hash());
        assert_eq!(observed.trace().render(), ordinary.trace().render());
        assert_eq!(observer.starts, tick + 1);
        assert_eq!(observer.finishes, observer.starts);
        assert!(!observer.active);
    }
    assert!(observer.phases > 0 && observer.rows > 0);
    // Four substeps per tick; faster motion only adds collision passes.
    assert!(observer.substeps >= 4 * 90);
}

#[test]
fn detailed_profile_counts_breach_and_fast_spin_work_without_changing_physics() {
    use arpg_sim::{PhysicsCounter, PhysicsPhase, StepObserver};
    #[derive(Default)]
    struct Observer { active: bool, counts: [u64; PhysicsCounter::ALL.len()] }
    impl StepObserver for Observer {
        const PHYSICS_DETAILS: bool = true;
        fn crowd_started(&mut self) { assert!(!self.active); self.active = true; }
        fn crowd_finished(&mut self, _: arpg_sim::CrowdWork) { assert!(self.active); self.active = false; }
        fn physics_phase(&mut self, _: PhysicsPhase) { assert!(self.active); }
        fn physics_count(&mut self, count: PhysicsCounter, amount: u64) {
            assert!(self.active);
            self.counts[count as usize] += amount;
        }
    }
    let mut scene = arpg_game::GameScene::default();
    scene.engine.bodies.push(arpg_game::Placed {
        pos: (0.0, 4.0),
        what: ActorDefinition { physical: arpg_sim::Template::BODY, ..ActorDefinition::prop() },
    });
    scene.doors.push(arpg_game::DoorSpec {
        body: 0,
        panel: arpg_game::DoorPanel::new(2.0, 0.01, 0.0, -1.55, 1.55, 0.1).unwrap(),
    });
    let mut observed = Game::from_scene(&scene).unwrap();
    let mut plain = Game::from_scene(&scene).unwrap();
    for game in [&mut observed, &mut plain] {
        let owner = game.scene_instances().next().unwrap().0;
        let panel = game.scene_bodies(owner).unwrap()[0];
        assert!(game.apply_impulse_at(panel, glam::Vec2::new(1.0, 4.0),
            arpg_sim::Impulse::try_from((0.0, -4.8)).unwrap()));
    }
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    let idle = Intent::new(MoveDir::NONE, AimDir::FORWARD, false);
    let mut observer = Observer::default();
    for _ in 0..4 {
        observed.step_observed(dt, idle, &mut observer);
        plain.step(dt, idle);
        assert!(!observer.active);
        assert_eq!(observed.hash(), plain.hash());
        assert_eq!(observed.trace().render(), plain.trace().render());
    }
    assert_eq!(observer.counts[PhysicsCounter::LockBreaks as usize], 1);
    // The breached panel's corner outruns one pass's reach: the tick splits.
    assert!(observer.counts[PhysicsCounter::CollisionPasses as usize] > 4);
    assert!(observer.counts[PhysicsCounter::Substeps as usize] >= 16);
}
