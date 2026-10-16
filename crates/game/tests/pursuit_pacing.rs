//! Independent pace changes through Game, including physical and lifetime boundaries.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{ActionDefinition, ActionKind, ActorDefinition, Game, GameScene, Placed, Scene};
use arpg_sim::{Accumulator, Dt, EntityId, Impulse};
use glam::Vec2;

fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}

fn scene() -> GameScene {
    Scene {
        bodies: vec![
            Placed { pos: (60.0, 0.0), what: ActorDefinition::hostile() },
            Placed { pos: (-60.0, 0.0), what: ActorDefinition::hostile() },
        ],
        ..Default::default()
    }.into()
}

fn speed(game: &Game, id: EntityId) -> f32 {
    game.motion(id).unwrap().velocity().length()
}

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.00001, "{actual} != {expected}");
}

#[test]
fn creatures_switch_at_staggered_boundaries_and_accelerate_through_both_paces() {
    let setup = scene();
    let mut game = Game::from_scene(&setup).unwrap();
    let mut replay = Game::from_scene(&setup).unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    // Identities #1v1/#2v1 have cycle offsets 333/150. Their first
    // transitions fall at ticks 27/30, then every 180 simulation ticks.
    for tick in 0..720 {
        step(&mut game);
        step(&mut replay);
        assert_eq!(game.hash(), replay.hash(), "tick {tick}");
        let expected = match tick {
            10 | 26 => Some((3.125, 1.875)),
            27 => Some((2.3125, 1.875)),
            28 | 29 => Some((1.875, 1.875)),
            30 => Some((1.875, 2.1875)),
            31 => Some((1.875, 2.46875)),
            32 => Some((1.875, 2.721875)),
            33 => Some((1.875, 2.9496875)),
            34 | 206 => Some((1.875, 3.125)),
            207 => Some((2.1875, 3.125)),
            209 => Some((2.721875, 3.125)),
            210 => Some((2.9496875, 2.3125)),
            211 => Some((3.125, 1.875)),
            _ => None,
        };
        if let Some((first, second)) = expected {
            near(speed(&game, ids[0]), first);
            near(speed(&game, ids[1]), second);
        }
        if tick == 27 {
            let mut report = Report::default();
            game.report(&mut report);
            let report = report.finish();
            assert!(report.contains("\"vary_speed\":true"));
            assert!(report.contains("\"move_speed\":1.8750"));
        }
    }
}

#[test]
fn definition_edits_and_impulses_use_the_same_bounded_motor() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let id = game.action_statuses().next().unwrap().body;
    for _ in 0..27 { step(&mut game); }
    let mut steady = ActorDefinition::hostile();
    let ActionDefinition::Move(movement) = &mut steady.actions[0] else { panic!("move preset") };
    movement.vary_speed = false;
    let before = game.hash();
    game.assign_behavior(id, steady).unwrap();
    assert_ne!(game.hash(), before);
    step(&mut game); // Tick 27 would have switched to walking without the edit.
    near(speed(&game, id), 3.125);
    game.assign_behavior(id, ActorDefinition::hostile()).unwrap();
    step(&mut game);
    near(speed(&game, id), 2.3125);
    step(&mut game);
    near(speed(&game, id), 1.875);
    assert!(game.apply_impulse(id, Impulse::try_from((0.0, 8.0)).unwrap()));
    step(&mut game);
    assert!(speed(&game, id) > 6.0, "switching pace must not erase a physical impulse");
    for _ in 0..30 { step(&mut game); }
    near(speed(&game, id), 1.875);
}

#[test]
fn removal_refusals_eviction_and_restart_preserve_pace_lifetimes() {
    let setup = scene();
    let mut game = Game::from_scene(&setup).unwrap();
    let mut control = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let ids = game.scene_bodies(owner).unwrap().to_vec();
    for _ in 0..25 { step(&mut game); step(&mut control); }
    assert!(game.despawn_body(ids[0]));
    let replacement = game.place(Vec2::new(0.0, -60.0), ActorDefinition::hostile()).unwrap();
    assert_ne!(replacement, ids[0]);
    assert_eq!(replacement.diagnostic_identity(), (1, 2));
    let ticket = game.request_spawn(Vec2::new(60.0, 60.0), ActorDefinition::hostile()).unwrap();
    let mut invalid = ActorDefinition::hostile();
    let ActionDefinition::Move(movement) = &mut invalid.actions[0] else { panic!("move preset") };
    movement.speed = f32::NAN;
    let before = game.hash();
    assert!(game.assign_behavior(ids[0], ActorDefinition::hostile()).is_err());
    assert!(game.assign_behavior(ids[1], invalid.clone()).is_err());
    let mut bad_scene = setup.clone();
    bad_scene.engine.bodies[0].what = invalid;
    assert!(game.start_scene(&bad_scene).is_err());
    assert_eq!(game.hash(), before);
    step(&mut game);
    step(&mut control);
    assert_eq!(game.spawn_outcomes()[0].ticket, ticket);
    let newborn = game.spawn_outcomes()[0].entity.unwrap();
    near(speed(&game, newborn), 0.5);
    for _ in 0..360 {
        step(&mut game);
        step(&mut control);
        assert_eq!(game.body_pos(ids[1]), control.body_pos(ids[1]));
        assert_eq!(game.motion(ids[1]).unwrap().velocity(), control.motion(ids[1]).unwrap().velocity());
        if game.tick() == 171 {
            // #1v2's offset is 22: it starts running at tick 158. The retired
            // #1v1 would still be walking until tick 207.
            near(speed(&game, replacement), 3.125);
        }
    }
    assert!(game.evict_scene(owner));
    assert!(game.action_status(ids[1]).is_none());
    game.restart().unwrap();
    let mut fresh = Game::from_scene(&setup).unwrap();
    assert_eq!(game.hash(), fresh.hash());
    for _ in 0..370 {
        step(&mut game);
        step(&mut fresh);
        assert_eq!(game.hash(), fresh.hash());
    }
}

#[test]
fn committed_attacks_keep_movement_control_across_a_pace_change() {
    let setup: GameScene = Scene {
        bodies: vec![Placed { pos: (0.0, 1.4), what: ActorDefinition::attacker() }],
        ..Default::default()
    }.into();
    let mut game = Game::from_scene(&setup).unwrap();
    let id = game.action_statuses().next().unwrap().body;
    let origin = game.body_pos(id).unwrap();
    for tick in 0..84 {
        step(&mut game);
        assert_eq!(game.action_status(id).unwrap().kind, ActionKind::Swipe, "tick {tick}");
        assert_eq!(game.body_pos(id), Some(origin));
        near(speed(&game, id), 0.0);
        assert_eq!(game.player_health(), if tick < 36 { 5 } else { 4 });
    }
}
