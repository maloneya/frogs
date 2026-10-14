//! Shared physical mechanisms at the adapter boundary preserve the engine's
//! tick history and physical output. Gameplay reports are checked by lifecycle tests.

use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{Game, ActorDefinition, Scene, Placed, BodyGrid, SourceSpec};
use arpg_sim::{
    Accumulator, Alpha, AttackProfile, Condition, Dt, Event, Impulse, World,
};
use glam::{Vec2, Vec3};

fn physical(scene:&Scene)->arpg_sim::Scene {
    arpg_sim::Scene {
        joints: vec![],
        name:scene.name.clone(), walls:scene.walls.clone(),
        bodies:scene.bodies.iter().map(|b|arpg_sim::Placed{pos:b.pos,what:b.what.physical}).collect(),
        grids:scene.grids.iter().map(|g|arpg_sim::BodyGrid{origin:g.origin,columns:g.columns,rows:g.rows,spacing:g.spacing,what:g.what.physical}).collect(),
        sources:scene.sources.iter().map(|s|arpg_sim::SourceSpec{enabled:s.enabled,pos:s.pos,radius:s.radius,every:s.every,when:s.when,what:s.what.physical}).collect(),
    }
}

fn assert_same(game: &Game, world: &World) {
    assert_eq!(game.tick(), world.tick());
    assert_eq!(game.engine_hash(), world.hash(), "state differs at tick {}", game.tick());
    assert_eq!(game.trace().render(), world.trace().render());
    let mut game_report = Report::default();
    let mut engine_report = Report::default();
    game.report(&mut game_report);
    world.report(&mut engine_report);
    // Game reports the complete engine object first, then appends gameplay
    // fields. Compare that prefix without mirroring the gameplay schema.
    let engine = engine_report.finish();
    let prefix = format!("{},", engine.strip_suffix('}').unwrap());
    assert!(game_report.finish().starts_with(&prefix));
}

#[test]
fn construction_matches_the_engine_scene_loading_path() {
    assert_same(&Game::default(), &World::default());
    assert_same(&Game::empty(), &World::empty());
    let scene = Scene { name: "authored crowd".into(), grids: vec![BodyGrid {
        origin: (-10.85, -10.85), columns: 32, rows: 32, spacing: 0.7, what: ActorDefinition::passive_hostile(),
    }], ..Default::default() };
    let world = World::from_scene(&physical(&scene)).unwrap();
    assert_same(&Game::from_scene(&scene.into()).unwrap(), &world);
}

#[test]
fn mixed_inputs_preserve_ticks_trace_reports_and_interpolation() {
    let scene = Scene {
        walls: Vec::new(),
        name: "entry point compatibility".into(),
        bodies: vec![
            Placed { pos: (1.0, 0.0), what: ActorDefinition::prop().interactive() },
            Placed { pos: (0.0, 1.0), what: ActorDefinition::passive_hostile() },
        ],
        sources: vec![SourceSpec {
            enabled: true,
            pos: (4.0, 4.0), radius: 1.0, every: 12,
            when: Condition::FewerThan(8), what: ActorDefinition::passive_hostile(),
        }],
        grids: vec![],
    };
    let mut game = Game::from_scene(&scene.clone().into()).unwrap();
    let mut world = World::from_scene(&physical(&scene)).unwrap();
    assert_same(&game, &world);
    let mut clock = Accumulator::default();
    let mut stepped = 0;

    // Includes frames with no tick and frames with several. Both adapters must
    // preserve the same fixed-tick boundary and interpolation remainder.
    for frame in 0..40 {
        let elapsed = [0.25, 0.25, 1.5, 3.0][frame % 4] * Dt::SECS;
        for dt in clock.pending(elapsed) {
            if stepped == 8 {
                let impulse = Impulse::try_from((2.0, -1.0)).unwrap();
                assert!(game.apply_impulse(game.player_id(), impulse));
                assert!(world.apply_impulse(world.player_id(), impulse));
                assert!(game.request_spawn(Vec2::new(-3.0, 2.0), ActorDefinition::passive_hostile()).is_some());
                assert!(world.request_spawn(Vec2::new(-3.0, 2.0), ActorDefinition::passive_hostile().physical).is_some());
            }
            if stepped == 20 {
                game.set_attack_profile(AttackProfile::Slam);
                world.set_attack_profile(AttackProfile::Slam);
            }
            let direction = if stepped < 12 { Vec3::ZERO } else { Vec3::Z };
            let intent = Intent::new(MoveDir::new(direction), AimDir::FORWARD, stepped % 24 == 0)
                .with_interact(stepped == 0);
            game.step(dt, intent);
            world.step(dt, intent);
            stepped += 1;
            assert_same(&game, &world);
        }

        let before = game.hash();
        for alpha in [Alpha::ZERO, clock.alpha(), Alpha::ONE] {
            let actual = game.player_presentation(alpha);
            let expected = world.player_presentation(alpha);
            assert_eq!(actual.id(), expected.id());
            assert_eq!(actual.ground_position(), expected.ground_position());
            assert_eq!(actual.facing(), expected.facing());
            assert_eq!(actual.displacement(), expected.displacement());
            assert_eq!(actual.attack().phase, expected.attack().phase);
            let enemies = |p: arpg_sim::CharacterPresentation| (p.id(), p.ground_position(), p.displacement());
            assert_eq!(game.character_presentations(alpha).map(enemies).collect::<Vec<_>>(), world.character_presentations(alpha).map(enemies).collect::<Vec<_>>());
            let props = |p: arpg_sim::PropPresentation| (p.id(), p.ground_position(), p.interaction());
            assert_eq!(game.prop_presentations(alpha).map(props).collect::<Vec<_>>(), world.prop_presentations(alpha).map(props).collect::<Vec<_>>());
        }
        assert_eq!(game.hash(), before, "presentation must not change playable state");
    }

    assert!(stepped > 24, "exercise both swings and the tuning change");
    assert!(game.trace().iter().any(|(_, event)| matches!(event, Event::Activated { .. })));
    assert!(game.trace().iter().any(|(_, event)| matches!(event, Event::Fired { .. })));
    assert!(game.enemy_count() > 1, "the source and queued request must actually spawn");
}

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
        let before = observed.enemy_count() as u64;
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
