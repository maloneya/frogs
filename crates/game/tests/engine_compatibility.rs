//! Compatibility at the adapter boundary: the game must preserve the engine's
//! tick history and physical output. Gameplay reports are checked by lifecycle tests.

use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::Game;
use arpg_sim::{
    Accumulator, Alpha, AttackProfile, Condition, Dt, Event, Impulse, Placed,
    Scene, SourceSpec, Template, World,
};
use glam::{Vec2, Vec3};

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
    let scene = Scene::boot();
    let world = World::from_scene(&scene).unwrap();
    assert_same(&Game::from_scene(&scene.clone().into()).unwrap(), &world);
}

#[test]
fn mixed_inputs_preserve_ticks_trace_reports_and_interpolation() {
    let scene = Scene {
        walls: Vec::new(),
        name: "entry point compatibility".into(),
        bodies: vec![
            Placed { pos: (1.0, 0.0), what: Template::BLOCK.interactive() },
            Placed { pos: (0.0, 1.0), what: Template::BODY },
        ],
        sources: vec![SourceSpec {
            enabled: true,
            pos: (4.0, 4.0), radius: 1.0, every: 12,
            when: Condition::FewerThan(8), what: Template::BODY.seeking(),
        }],
        grids: vec![],
    };
    let mut game = Game::from_scene(&scene.clone().into()).unwrap();
    let mut world = World::from_scene(&scene).unwrap();
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
                assert!(game.request_spawn(Vec2::new(-3.0, 2.0), Template::BODY).is_some());
                assert!(world.request_spawn(Vec2::new(-3.0, 2.0), Template::BODY).is_some());
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
            let enemies = |p: arpg_sim::EnemyPresentation| (p.id(), p.ground_position(), p.displacement());
            assert_eq!(game.enemy_presentations(alpha).map(enemies).collect::<Vec<_>>(), world.enemy_presentations(alpha).map(enemies).collect::<Vec<_>>());
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
    struct Observer { active: bool, starts: usize, finishes: usize, work: arpg_sim::CrowdWork }
    impl arpg_sim::StepObserver for Observer {
        fn crowd_started(&mut self) {
            assert!(!self.active);
            self.active = true;
            self.starts += 1;
        }
        fn crowd_finished(&mut self, work: arpg_sim::CrowdWork) {
            assert!(self.active);
            self.active = false;
            self.finishes += 1;
            self.work = work;
        }
    }
    let scene = arpg_game::GameScene {
        engine: Scene {
            grids: vec![arpg_sim::BodyGrid {
                origin: (-3.0, -3.0), columns: 10, rows: 10, spacing: 0.6,
                what: Template::BODY.seeking(),
            }], ..Default::default()
        }, ..Default::default()
    };
    let mut observed = Game::from_scene(&scene).unwrap();
    let mut ordinary = Game::from_scene(&scene).unwrap();
    let mut observer = Observer::default();
    for tick in 0..90 {
        let before = observed.enemy_count() as u64;
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let intent = Intent::new(MoveDir::new(Vec3::X), AimDir::FORWARD, tick % 25 == 0);
        observed.step_observed(dt, intent, &mut observer);
        ordinary.step(dt, intent);
        assert_eq!(observer.work.possible_pairs(), before * before.saturating_sub(1) / 2);
        assert!(observer.work.candidates() <= observer.work.possible_pairs());
        assert_eq!(observer.work.contacts(), observed.crowd_contacts());
        assert_eq!(observed.hash(), ordinary.hash());
        assert_eq!(observed.trace().render(), ordinary.trace().render());
        assert_eq!(observer.starts, tick + 1);
        assert_eq!(observer.finishes, tick + 1);
    }
}
