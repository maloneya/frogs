//! Gait timing, identity and lifecycle through the same Game as the live app.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::Game;
use arpg_sim::{Accumulator, Dt, Event, Gait, Placed, Scene, Template};
use glam::Vec2;

fn step(game: &mut Game) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}

fn scene() -> arpg_game::GameScene {
    Scene {
        bodies: vec![
            Placed { pos: (20.0, 0.0), what: Template::BODY.seeking() },
            Placed { pos: (-20.0, 0.0), what: Template::BODY.seeking() },
            Placed { pos: (0.0, 20.0), what: Template::BODY.seeking() },
        ],
        ..Scene::default()
    }.into()
}

#[test]
fn staggered_choices_change_at_their_boundary_and_accelerate_instead_of_snapping() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let ids = game.scene_bodies(owner).unwrap().to_vec();
    assert_eq!(ids.iter().map(|&id| game.gait(id)).collect::<Vec<_>>(),
        [Some(Gait::Run), Some(Gait::Sprint), Some(Gait::Walk)]);
    for _ in 0..27 { step(&mut game); }
    assert_eq!(game.gait(ids[0]), Some(Gait::Run));
    assert_eq!(game.motion(ids[0]).unwrap().velocity(), Vec2::new(-2.5, 0.0));
    step(&mut game); // Tick 27: 2.5*.9+.5 = 2.75, not an instant 4m/s.
    assert_eq!(game.gait(ids[0]), Some(Gait::Sprint));
    assert!((game.motion(ids[0]).unwrap().velocity().x + 2.75).abs() < 0.00001);
    assert_eq!(game.gait(ids[1]), Some(Gait::Sprint));
    assert!(game.trace().iter().any(|(tick, event)| tick == 27
        && event == Event::GaitsChanged { walking: 0, running: 0, sprinting: 1 }));
    step(&mut game);
    step(&mut game);
    assert_eq!(game.gait(ids[1]), Some(Gait::Sprint));
    step(&mut game); // Tick 30: 4*.9-.5 = 3.1 while requesting Run.
    assert_eq!(game.gait(ids[1]), Some(Gait::Run));
    assert!((game.motion(ids[1]).unwrap().velocity().x - 3.1).abs() < 0.00001);
    let mut report = Report::default();
    game.report(&mut report);
    let text = report.finish();
    assert!(text.contains("\"gaits\""));
    assert!(text.contains("\"gait\":\"sprint\",\"desired_speed\":4"));
    let before = game.hash();
    game.clear_trace();
    assert_eq!(game.hash(), before, "diagnostic transitions cannot drive gait choices");
}

#[test]
fn row_moves_retirement_eviction_and_restart_preserve_the_choice_contract() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let mut control = Game::from_scene(&scene()).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let ids = game.scene_bodies(owner).unwrap().to_vec();
    assert!(game.despawn_body(ids[0]));
    assert_eq!(game.gait(ids[0]), None);
    let replacement = game.place(Vec2::new(0.0, -30.0), Template::BODY).unwrap();
    assert!(!game.add_seek(ids[0]), "stale identity must not grant a motor");
    assert_eq!(game.gait(replacement), None);
    assert!(!game.add_seek(game.player_id()));
    let block = game.place(Vec2::new(30.0, 30.0), Template::BLOCK).unwrap();
    assert!(!game.add_seek(block));
    for _ in 0..90 {
        step(&mut game);
        step(&mut control);
        for &id in &ids[1..] {
            assert_eq!(game.gait(id), control.gait(id));
            assert_eq!(game.body_pos(id), control.body_pos(id));
            assert_eq!(game.motion(id).unwrap().velocity(), control.motion(id).unwrap().velocity());
        }
    }
    assert!(game.evict_scene(owner));
    for id in ids { assert_eq!(game.gait(id), None); }
    game.restart().unwrap();
    let mut fresh = Game::from_scene(&scene()).unwrap();
    assert_eq!(game.hash(), fresh.hash());
    for _ in 0..220 {
        step(&mut game);
        step(&mut fresh);
        assert_eq!(game.hash(), fresh.hash());
    }
}
