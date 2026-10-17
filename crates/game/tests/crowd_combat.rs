//! Crowd clearing and fast shared motion through the playable Game boundary.
use arpg_game::{Scene, BodyGrid, ActorDefinition};
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::Game;
use arpg_sim::{Accumulator, Dt, Impulse, Wall};
use arpg_game::{AttackProfile};
use glam::{Vec2, Vec3};

#[test]
fn a_sweeping_beam_can_remove_most_of_a_thousand_seekers_in_thirty_seconds() {
    let scene = Scene {
        grids: vec![BodyGrid { origin: (-10.85, 3.0), columns: 32, rows: 32,
            spacing: 0.7, what: ActorDefinition::hostile() }],
        ..Default::default()
    }.into();
    let mut game = Game::from_scene(&scene).unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    // A six-second revolution gives a close target several damage pulses.
    // Prediction: at least half the incoming horde is cleared by 15 seconds,
    // and at least three quarters by 30, without moving or spawning support.
    for tick in 0..1800 {
        let angle = tick as f32 * core::f32::consts::TAU / 360.0;
        let aim = AimDir::new(Vec3::new(angle.sin(), 0.0, angle.cos())).unwrap();
        game.step(dt, Intent::new(MoveDir::NONE, aim, false).with_attack_held(true));
        if tick == 899 { assert!(game.enemy_count() <= 512, "remaining: {}", game.enemy_count()); }
    }
    assert!(game.enemy_count() <= 256, "remaining: {}", game.enemy_count());
}

#[test]
fn fast_motion_shares_momentum_but_misses_and_walls_do_not() {
    for (offset, distance, blocked) in [(0.0, 1.0, false), (0.0, 0.5, false), (0.6, 1.0, false), (0.0, 1.0, true)] {
        let mut scene = Scene::default();
        if blocked {
            scene.walls.push(Wall::new(Vec2::new(0.0, 5.5), Vec2::new(4.0, 0.1), 0.0).unwrap());
        }
        let mut game = Game::from_scene(&scene.into()).unwrap();
        let a = game.place(Vec2::new(0.0, 5.0), ActorDefinition::passive_hostile()).unwrap();
        let b = game.place(Vec2::new(offset, 5.0 + distance), ActorDefinition::passive_hostile()).unwrap();
        assert!(game.apply_impulse(a, Impulse::try_from((0.0, 72.0)).unwrap()));
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
        if !blocked && offset == 0.0 {
            // Speculative contact closes the gap, then relaxation removes the
            // closing speed: equal masses share the momentum, then passive drag.
            let (va, vb) = (game.motion(a).unwrap().velocity().y, game.motion(b).unwrap().velocity().y);
            assert!((va + vb - 72.0 * 0.9).abs() < 0.001, "momentum {va} + {vb}");
            assert!((va - vb).abs() < 0.01, "an inelastic contact leaves no closing speed: {va}, {vb}");
            let gap = game.body_pos(b).unwrap().z - game.body_pos(a).unwrap().z;
            assert!(gap >= 0.5 - 0.005, "the impact crossed the body: {gap}");
            assert!(0.5 * (va * va + vb * vb) <= 0.5 * 72.0 * 72.0);
        } else {
            assert_eq!(game.motion(b).unwrap().velocity(), Vec2::ZERO);
        }
    }
}
