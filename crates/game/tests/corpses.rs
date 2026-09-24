//! Corpse admission and scene lifecycle through the public Game boundary.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{Game, GameScene, MAX_CORPSES, CORPSE_LIFETIME_TICKS};
use arpg_sim::{Accumulator, AttackProfile, Dt, Placed, Scene, Template};
use glam::Vec2;

fn step(game: &mut Game, held: bool) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(held));
}
fn kill_line(game: &mut Game) {
    game.set_attack_profile(AttackProfile::Disintegrate);
    for _ in 0..25 { step(game, true); }
    assert_eq!(game.enemy_count(), 0);
    step(game, false);
}
fn scene() -> GameScene {
    Scene { bodies: vec![Placed { pos: (0.0, 3.0), what: Template::BODY }], ..Scene::default() }.into()
}

#[test]
fn full_budget_skips_deaths_and_expiry_reopens_admission() {
    let mut game = Game::empty();
    for batch in 0..=MAX_CORPSES / 8 {
        for i in 0..8 { game.place(Vec2::new(0.0, 2.0 + i as f32), Template::BODY).unwrap(); }
        kill_line(&mut game);
        assert_eq!(game.corpses().len(), ((batch + 1) * 8).min(MAX_CORPSES));
    }
    let newest = game.corpses().last().unwrap().remaining_ticks();
    for _ in 0..newest { step(&mut game, false); }
    assert!(game.corpses().is_empty());
    game.place(Vec2::new(0.0, 3.0), Template::BODY).unwrap();
    kill_line(&mut game);
    assert_eq!(game.corpses().len(), 1);
    assert_eq!(game.corpses()[0].remaining_ticks(), CORPSE_LIFETIME_TICKS - 1);
}

#[test]
fn corpse_lifetime_obeys_scene_ownership_refusal_and_restart() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let old = game.scene_bodies(owner).unwrap()[0];
    kill_line(&mut game);
    assert_eq!(game.corpses()[0].id(), old);
    assert!(!game.despawn_body(old));
    let replacement = game.place(Vec2::new(0.0, 8.0), Template::BODY).unwrap();
    assert_ne!(replacement, old);
    assert_eq!(game.corpses()[0].id(), old);
    let before = game.hash();
    let mut bad = scene();
    bad.engine.bodies[0].pos.0 = f32::NAN;
    assert!(game.load_scene(&bad).is_err());
    assert!(game.start_scene(&bad).is_err());
    assert_eq!(game.hash(), before);
    let unrelated = game.load_scene(&Scene::default().into()).unwrap();
    assert!(game.evict_scene(unrelated));
    assert_eq!(game.corpses().len(), 1);
    let mut out = Report::default();
    game.report(&mut out);
    let report = out.finish();
    assert!(report.contains("\"corpse_count\":1"));
    assert!(report.contains(&format!("\"corpses\":{{\"{old}\":{{\"scene\":\"{owner}\"")));
    assert!(game.evict_scene(owner));
    assert!(game.corpses().is_empty());
    game.restart().unwrap();
    kill_line(&mut game);
    game.restart().unwrap();
    assert!(game.corpses().is_empty());
    assert_eq!(game.hash(), Game::from_scene(&scene()).unwrap().hash());
    let live = game.scene_bodies(game.scene_instances().next().unwrap().0).unwrap()[0];
    assert!(game.despawn_body(live));
    step(&mut game, false);
    assert!(game.corpses().is_empty(), "administrative despawn is not a death");
}
