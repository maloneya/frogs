//! Physical player control uses the same bounded motor and mass as other bodies.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{ActorDefinition, Game, GameScene, Placed, Scene};
use arpg_sim::{Accumulator, Dt, Impulse, Mass, Template};
use arpg_game::{TuningField};
use glam::{Vec2, Vec3};

fn step(game: &mut Game, direction: Vec3) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::new(direction), AimDir::FORWARD, false),
    );
}

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
}

#[test]
fn input_accelerates_then_release_brakes_with_bounded_effort() {
    let mut game = Game::empty();
    // 60m/s² contributes at most 1m/s per tick; carried speed retains 90%.
    // Each tick's whole change ramps in over four substeps: a tick covers
    // (v + 5/8 change) dt.
    for speed in [1.0, 1.9, 2.71] {
        step(&mut game, Vec3::X);
        near(game.motion(game.player_id()).unwrap().velocity().x, speed);
    }
    near(game.player_pos().x, (0.625 + 1.5625 + 2.40625) / 60.0);
    for speed in [1.439, 0.2951, 0.0] {
        step(&mut game, Vec3::ZERO);
        near(game.motion(game.player_id()).unwrap().velocity().x, speed);
    }
    near(
        game.player_pos().x,
        (4.59375 + 1.915625 + 0.724_062_5 + 0.110_662_5) / 60.0,
    );
    let stopped = game.player_pos();
    for _ in 0..10 {
        step(&mut game, Vec3::ZERO);
    }
    assert_eq!(game.player_pos(), stopped);
}

#[test]
fn impulse_and_control_compose_and_acceleration_tuning_changes_the_available_effort() {
    let mut game = Game::empty();
    let player = game.player_id();
    // 100kg m/s / 20kg = 5m/s. Drag leaves 4.5; braking removes only 1.
    assert!(game.apply_impulse(player, Impulse::try_from((100.0, 0.0)).unwrap()));
    step(&mut game, Vec3::ZERO);
    near(game.motion(player).unwrap().velocity().x, 3.5);
    near(game.player_pos().x, (5.0 - 0.625 * 1.5) / 60.0);
    // A stronger motor can oppose that remaining motion in this tick.
    game.set_tuning(
        game.tuning()
            .with(TuningField::PlayerAcceleration, 300.0)
            .unwrap(),
    );
    step(&mut game, Vec3::ZERO);
    assert_eq!(game.motion(player).unwrap().velocity(), Vec2::ZERO);
    step(&mut game, Vec3::X);
    near(game.motion(player).unwrap().velocity().x, 5.0);
    game.set_tuning(game.tuning().with(TuningField::PlayerSpeed, 2.0).unwrap());
    step(&mut game, Vec3::X);
    near(game.motion(player).unwrap().velocity().x, 2.0);
}

#[test]
fn mass_is_a_body_property_independent_of_role_and_player_mass_is_tunable() {
    let mut game = Game::empty();
    let mut light = ActorDefinition::passive_hostile();
    light.physical = Template::BODY.with_mass(Mass::try_from(2.0).unwrap());
    let mut heavy = light.clone();
    heavy.physical = Template::BODY.with_mass(Mass::try_from(4.0).unwrap());
    let light = game.place(Vec2::new(-10.0, 0.0), light).unwrap();
    let heavy = game.place(Vec2::new(10.0, 0.0), heavy).unwrap();
    for id in [light, heavy] {
        assert!(game.apply_impulse(id, Impulse::try_from((8.0, 0.0)).unwrap()));
    }
    near(game.motion(light).unwrap().velocity().x, 4.0);
    near(game.motion(heavy).unwrap().velocity().x, 2.0);
    game.set_tuning(game.tuning().with(TuningField::PlayerMass, 2.0).unwrap());
    let player = game.player_id();
    assert!(game.apply_impulse(player, Impulse::try_from((8.0, 0.0)).unwrap()));
    near(game.motion(player).unwrap().velocity().x, 4.0);
    let before = game.hash();
    for invalid in [
        f32::NAN,
        f32::INFINITY,
        0.0,
        -1.0,
        Mass::MIN / 2.0,
        Mass::MAX * 2.0,
    ] {
        assert!(Mass::try_from(invalid).is_err());
    }
    assert_eq!(game.hash(), before);
    assert!(game.despawn_body(light));
    assert!(game.motion(light).is_none());
    assert!(!game.apply_impulse(light, Impulse::try_from((8.0, 0.0)).unwrap()));
    near(game.motion(heavy).unwrap().inverse_mass(), 0.25);
}

#[test]
fn authored_mass_survives_restart_and_scene_lifetime_and_participates_in_hash_and_report() {
    let mut definition = ActorDefinition::passive_hostile();
    definition.physical = definition.physical.with_mass(Mass::try_from(4.0).unwrap());
    let scene: GameScene = Scene {
        bodies: vec![Placed {
            pos: (10.0, 0.0),
            what: definition,
        }],
        ..Scene::default()
    }
    .into();
    let mut game = Game::from_scene(&scene).unwrap();
    let initial = game.hash();
    let owner = game.scene_instances().next().unwrap().0;
    let body = game.scene_bodies(owner).unwrap()[0];
    near(game.motion(body).unwrap().inverse_mass(), 0.25);
    let mut report = Report::default();
    game.report(&mut report);
    assert!(report.finish().contains("\"inverse_mass\":0.25"));
    assert!(game.evict_scene(owner));
    assert!(game.motion(body).is_none());
    game.restart().unwrap();
    assert_eq!(game.hash(), initial);
    let mut other = scene;
    other.engine.bodies[0].what.physical =
        other.engine.bodies[0].what.physical.with_mass(Mass::UNIT);
    assert_ne!(Game::from_scene(&other).unwrap().hash(), initial);
}

const DEEPEST: f32 = 1.0;

#[test]
fn player_motor_pushes_a_connected_crowd_with_bounded_compression_and_replayable_motion() {
    for direction in [Vec3::X, Vec3::new(1.0, 0.0, -1.0).normalize()] {
        let ground = Vec2::new(direction.x, direction.z);
        let scene: GameScene = Scene {
            bodies: [0.6, 1.1, 1.6]
                .map(|distance| Placed {
                    pos: (ground * distance).into(),
                    what: ActorDefinition::passive_hostile(),
                })
                .into(),
            ..Default::default()
        }
        .into();
        let mut game = Game::from_scene(&scene).unwrap();
        let mut replay = Game::from_scene(&scene).unwrap();
        let owner = game.scene_instances().next().unwrap().0;
        let bodies = game.scene_bodies(owner).unwrap().to_vec();
        let mut deepest = 0.0_f32;
        for tick in 0..120 {
            step(&mut game, direction);
            step(&mut replay, direction);
            assert_eq!(game.hash(), replay.hash(), "replay diverged at tick {tick}");
            assert!(game.all_positions_finite());
            let player = game.player_pos();
            let player = Vec2::new(player.x, player.z);
            let positions: Vec<_> = bodies
                .iter()
                .map(|&id| {
                    let position = game.body_pos(id).unwrap();
                    Vec2::new(position.x, position.z)
                })
                .collect();
            deepest = deepest.max(0.55 - positions[0].distance(player));
            for pair in positions.windows(2) {
                deepest = deepest.max(0.5 - pair[1].distance(pair[0]));
            }
            if tick == 2 {
                // The third tick reaches the touching chain. Momentum reaches
                // every body within the tick through the shared solve.
                for &id in &bodies {
                    assert!(game.motion(id).unwrap().velocity().dot(ground) > 0.0);
                }
                // Contact impulses conserve momentum; passive drag follows.
                let momentum =
                    20.0 * f64::from(
                        game.motion(game.player_id())
                            .unwrap()
                            .velocity()
                            .dot(ground),
                    ) + bodies
                        .iter()
                        .map(|&id| f64::from(game.motion(id).unwrap().velocity().dot(ground)) / 0.9)
                        .sum::<f64>();
                assert!((momentum - 20.0 * 2.71).abs() < 0.001);
            }
        }
        // Soft contacts compress under the 20kg player's full motor effort,
        // but only by millimetres.
        assert!(deepest < DEEPEST, "deepest overlap {deepest}m");
        assert!(
            game.player_pos().dot(direction) > 10.0,
            "a bounded motor must make progress through a movable crowd"
        );
        for &id in &bodies {
            assert!(game.body_pos(id).unwrap().dot(direction) > 10.0);
            assert!(game.motion(id).unwrap().velocity().dot(ground) > 0.0);
        }
    }
}
