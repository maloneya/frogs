//! Predicted leap and crowd-admission outcomes through Game, independent of playable scenes.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{
    ActionKind, ActorDefinition, Game, GameScene, MAX_COMBAT_COMMITMENTS, PLAYER_MAX_HEALTH,
    Placed, Scene,
};
use arpg_sim::{Accumulator, AttackPhase, Dt, Wall};
use glam::{Vec2, Vec3};

fn scene(positions: &[(f32, f32)]) -> GameScene {
    GameScene {
        engine: Scene {
            bodies: positions
                .iter()
                .map(|&pos| Placed {
                    pos,
                    what: ActorDefinition::attacker(),
                })
                .collect(),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn step(game: &mut Game, movement: Vec3, beam: bool) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(
        dt,
        Intent::new(MoveDir::new(movement), AimDir::FORWARD, false).with_attack_held(beam),
    );
}

#[test]
fn leap_warns_flies_lands_once_then_switches_to_a_close_swipe() {
    let description = scene(&[(0.0, 4.0)]);
    let mut game = Game::from_scene(&description).unwrap();
    let mut replay = Game::from_scene(&description).unwrap();
    for tick in 0..=168 {
        step(&mut game, Vec3::ZERO, false);
        step(&mut replay, Vec3::ZERO, false);
        assert_eq!(game.hash(), replay.hash());
        let attack = game.action_statuses().next().unwrap();
        if tick < 132 {
            assert_eq!(attack.kind, ActionKind::Leap);
            let phase = if tick < 42 {
                AttackPhase::Startup
            } else if tick < 78 {
                AttackPhase::Active
            } else {
                AttackPhase::Recovery
            };
            assert_eq!(attack.phase, phase, "tick {tick}");
            assert_eq!(attack.centre, Vec2::ZERO);
            if tick < 42 {
                assert_eq!(game.body_pos(attack.body).unwrap().z, 4.0);
            }
            if tick == 59 {
                assert!(game.body_pos(attack.body).unwrap().z < 3.0);
            }
        } else {
            assert_eq!(attack.kind, ActionKind::Swipe);
        }
        assert_eq!(
            game.player_health(),
            if tick < 78 {
                5
            } else if tick < 168 {
                4
            } else {
                3
            },
            "tick {tick}"
        );
    }
}

#[test]
fn dodging_during_windup_leaves_the_landing_point_behind() {
    let mut game = Game::from_scene(&scene(&[(0.0, 4.0)])).unwrap();
    for tick in 0..132 {
        step(
            &mut game,
            if (1..13).contains(&tick) {
                Vec3::X
            } else {
                Vec3::ZERO
            },
            false,
        );
        let attack = game.action_statuses().next().unwrap();
        assert_eq!(attack.centre, Vec2::ZERO);
        assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
        if tick == 77 {
            assert!(
                game.body_pos(attack.body).unwrap().z.abs() < 0.0001,
                "flight endpoint: {:?}",
                game.body_pos(attack.body)
            );
        }
    }
}

#[test]
fn walls_block_leap_admission_and_midflight_walls_prevent_remote_landing_hits() {
    let wall = Wall::new(Vec2::new(0.0, 2.0), Vec2::new(6.0, 0.1), 0.0).unwrap();
    let mut blocked = scene(&[(0.0, 4.0)]);
    blocked.engine.walls.push(wall);
    let mut game = Game::from_scene(&blocked).unwrap();
    for _ in 0..150 {
        step(&mut game, Vec3::ZERO, false);
        assert_eq!(
            game.action_statuses().next().unwrap().kind,
            ActionKind::Move
        );
        assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    }
    game.start_scene(&scene(&[(0.0, 4.0)])).unwrap();
    for _ in 0..43 {
        step(&mut game, Vec3::ZERO, false);
    }
    game.load_scene(&GameScene {
        engine: Scene {
            walls: vec![wall],
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    for _ in 43..132 {
        step(&mut game, Vec3::ZERO, false);
    }
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    assert!(
        game.body_pos(game.action_statuses().next().unwrap().body)
            .unwrap()
            .z
            > 2.0
    );
}

#[test]
fn defeat_during_flight_cancels_landing_and_restart_restores_fresh_state() {
    let description = scene(&[(0.0, 4.0)]);
    let mut game = Game::from_scene(&description).unwrap();
    let initial_hash = game.hash();
    game.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
    for tick in 0..100 {
        step(&mut game, Vec3::ZERO, tick >= 42);
        if tick >= 66 {
            assert_eq!(game.action_statuses().count(), 0);
        }
        assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    }
    game.restart().unwrap();
    // Attack selection is engine run state; restart restores the scene's default.
    assert_eq!(game.hash(), initial_hash);
    assert_eq!(
        game.action_statuses().next().unwrap().phase,
        AttackPhase::Idle
    );
}

#[test]
fn horde_commitments_are_spaced_bounded_and_eviction_cleans_only_the_owner() {
    let positions = [
        (0.0, 4.0),
        (4.0, 0.0),
        (0.0, -4.0),
        (-4.0, 0.0),
        (3.0, 3.0),
        (-3.0, -3.0),
    ];
    // Give the coordination test leap-only actors: eligibility stays stable
    // while slots are staggered, independently of pursuit tuning.
    let mut description = scene(&positions);
    for body in &mut description.engine.bodies {
        body.what.actions = vec![arpg_game::ActionDefinition::Leap(Default::default())];
        body.what.behavior = arpg_game::BehaviorNode::Action(0);
    }
    let mut game = Game::from_scene(&description).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let mut starts = Vec::new();
    for tick in 0..42 {
        step(&mut game, Vec3::ZERO, false);
        let committed: Vec<_> = game
            .action_statuses()
            .filter(|a| matches!(a.kind, ActionKind::Swipe | ActionKind::Leap))
            .collect();
        assert!(committed.len() <= MAX_COMBAT_COMMITMENTS);
        starts.extend(
            committed
                .iter()
                .filter(|a| a.phase == AttackPhase::Startup && a.elapsed == 0)
                .map(|_| tick),
        );
    }
    assert_eq!(starts, vec![0, 12, 24, 36]);
    assert_eq!(game.action_statuses().count(), 6);
    let extra = game.load_scene(&scene(&[(12.0, 0.0)])).unwrap();
    let survivor = game.scene_bodies(extra).unwrap()[0];
    assert!(game.evict_scene(owner));
    assert_eq!(game.action_statuses().next().unwrap().body, survivor);
    assert_eq!(game.action_statuses().count(), 1);
    assert!(game.despawn_body(survivor));
    assert_eq!(game.action_statuses().count(), 0);
}

#[test]
fn leap_cooldown_prevents_an_immediate_second_jump_at_a_new_midrange_target() {
    let mut game = Game::from_scene(&scene(&[(0.0, 4.0)])).unwrap();
    let mut eligible_distance_observed = false;
    for tick in 0..240 {
        step(
            &mut game,
            if (1..13).contains(&tick) || (132..144).contains(&tick) {
                Vec3::X
            } else {
                Vec3::ZERO
            },
            false,
        );
        let attack = game.action_statuses().next().unwrap();
        if tick >= 132 {
            if attack.phase == AttackPhase::Startup && attack.elapsed == 0 {
                assert_eq!(attack.kind, ActionKind::Swipe);
            }
            let distance = game
                .body_pos(attack.body)
                .unwrap()
                .distance(game.player_pos());
            if (3.0..=7.0).contains(&distance) {
                eligible_distance_observed = true;
                assert_eq!(attack.kind, ActionKind::Move);
            }
        }
    }
    assert!(eligible_distance_observed);
}
