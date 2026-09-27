//! The first enemy-damage relationship through Game's public lifecycle.

use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{ActorDefinition, Placed, Scene, Game, GameScene, SceneError, PLAYER_MAX_HEALTH};
use arpg_sim::{Accumulator, Dt, };

fn attacker_scene(name: &str) -> GameScene {
    GameScene {
        engine: Scene {
            name: name.into(),
            bodies: vec![Placed { pos: (0.0, 0.55), what: ActorDefinition::attacker() }],
            ..Scene::default()
        },
        ..GameScene::default()
    }
}

fn step(game: &mut Game) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}

#[test]
fn rejected_action_definitions_preserve_the_live_game_and_accepted_work() {
    let mut game = Game::from_scene(&attacker_scene("first")).unwrap();
    let ticket = game.request_spawn(glam::Vec2::new(20.0, 0.0), ActorDefinition::hostile()).unwrap();
    let before = game.hash();
    let mut invalid = attacker_scene("bad action reference");
    invalid.engine.bodies[0].what.behavior = arpg_game::BehaviorNode::Action(99);
    assert!(matches!(game.load_scene(&invalid), Err(SceneError::Behavior(_))));
    assert_eq!(game.hash(), before);
    step(&mut game);
    assert_eq!(game.spawn_outcomes()[0].ticket, ticket);
    assert!(game.spawn_outcomes()[0].entity.is_some());
}

#[test]
fn stale_attackers_retire_and_restart_restores_health() {
    let scene = attacker_scene("first");
    let mut game = Game::from_scene(&scene).unwrap();
    for _ in 0..37 { step(&mut game); }
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH - 1);

    game.restart().unwrap();
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    let owner = game.scene_instances().next().unwrap().0;
    let body = game.scene_bodies(owner).unwrap()[0];
    assert!(game.despawn_body(body));
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);

    assert!(game.load_scene(&attacker_scene("replacement")).is_ok());
}

#[test]
fn windup_strike_and_recovery_have_exact_boundaries_and_one_hit() {
    use arpg_sim::AttackPhase;
    let mut game = Game::from_scene(&attacker_scene("timing")).unwrap();
    let mut replay = Game::from_scene(&attacker_scene("timing")).unwrap();
    for tick in 0..=120 {
        step(&mut game);
        step(&mut replay);
        assert_eq!(game.hash(), replay.hash());
        let status = game.action_statuses().next().unwrap();
        let age = tick % 84;
        let phase = if age < 36 { AttackPhase::Startup }
            else if age < 45 { AttackPhase::Active } else { AttackPhase::Recovery };
        assert_eq!(status.phase, phase, "tick {tick}");
        assert_eq!(game.player_health(), if tick < 36 { 5 } else if tick < 120 { 4 } else { 3 }, "tick {tick}");
    }
    let mut report = arpg_core::Report::default();
    game.report(&mut report);
    let report = report.finish();
    assert!(report.contains("\"phase\":\"active\""));
    assert!(report.contains("\"elapsed_ticks\":0"));
    assert!(report.contains("\"hit\":true"));
}

#[test]
fn sidestepping_the_telegraph_misses_and_the_enemy_does_not_track() {
    use glam::Vec3;
    let mut scene = attacker_scene("dodge");
    scene.engine.bodies[0].pos = (0.0, 1.4);
    let mut game = Game::from_scene(&scene).unwrap();
    step(&mut game);
    let committed = game.action_statuses().next().unwrap();
    let origin = game.body_pos(committed.body).unwrap();
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    for tick in 1..84 {
        game.step(dt, Intent::new(if tick < 13 { MoveDir::new(Vec3::X) } else { MoveDir::NONE }, AimDir::FORWARD, false));
        let status = game.action_statuses().next().unwrap();
        assert_eq!(status.direction, committed.direction);
        assert_eq!(game.body_pos(committed.body).unwrap(), origin, "no powered pursuit during commitment");
        assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    }
    step(&mut game);
    assert_eq!(game.action_statuses().next().unwrap().kind, arpg_game::ActionKind::Move);
    assert_ne!(game.body_pos(committed.body).unwrap(), origin, "pursuit resumes after recovery");
}

#[test]
fn walls_block_startup_and_a_defeated_attacker_cannot_finish_its_swing() {
    use glam::Vec2;
    let mut scene = attacker_scene("blocked");
    scene.engine.bodies[0].pos = (0.0, 1.4);
    scene.engine.walls.push(arpg_sim::Wall::new(Vec2::new(0.0, 0.7), Vec2::new(4.0, 0.1), 0.0).unwrap());
    let mut game = Game::from_scene(&scene).unwrap();
    for _ in 0..150 {
        step(&mut game);
        assert_eq!(game.action_statuses().next().unwrap().kind, arpg_game::ActionKind::Move);
        assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    }
    scene.engine.walls.clear();
    game.start_scene(&scene).unwrap();
    game.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    for tick in 0..84 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true));
        assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
        if tick < 24 { assert!(game.action_statuses().next().is_some()); }
        else { assert!(game.action_statuses().next().is_none()); }
    }
}

#[test]
fn eviction_and_recycled_slots_cannot_inherit_a_committed_swipe() {
    let mut game = Game::from_scene(&attacker_scene("eviction")).unwrap();
    step(&mut game);
    let status = game.action_statuses().next().unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    assert!(game.evict_scene(owner));
    assert!(game.action_statuses().next().is_none());
    assert!(!game.despawn_body(status.body));
    game.load_scene(&attacker_scene("replacement")).unwrap();
    assert_ne!(game.action_statuses().next().unwrap().body, status.body);
    for _ in 0..36 { step(&mut game); }
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    step(&mut game);
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH - 1);
    game.restart().unwrap();
    assert_eq!(game.action_statuses().next().unwrap().phase, arpg_sim::AttackPhase::Idle);
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
}

#[test]
fn a_wall_added_during_windup_blocks_damage_and_knockback_still_moves_a_held_attacker() {
    use glam::Vec2;
    let mut scene = attacker_scene("committed");
    scene.engine.bodies[0].pos = (0.0, 1.4);
    let mut game = Game::from_scene(&scene).unwrap();
    step(&mut game);
    game.load_scene(&GameScene {
        engine: Scene { walls: vec![arpg_sim::Wall::new(Vec2::new(0.0, 0.7), Vec2::new(4.0, 0.1), 0.0).unwrap()], ..Default::default() },
        ..Default::default()
    }).unwrap();
    for _ in 1..45 { step(&mut game); }
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    game.start_scene(&scene).unwrap();
    step(&mut game);
    let status = game.action_statuses().next().unwrap();
    assert!(game.apply_impulse(status.body, arpg_sim::Impulse::try_from((20.0, 0.0)).unwrap()));
    step(&mut game);
    assert!(game.body_pos(status.body).unwrap().x > 0.0);
    assert_eq!(game.action_statuses().next().unwrap().direction, status.direction);
    for _ in 2..45 { step(&mut game); }
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
}
