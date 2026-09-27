//! Corpse admission and scene lifecycle through the public Game boundary.
use arpg_game::{Placed, Scene, ActorDefinition};
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{Game, GameScene, MAX_CORPSES, CORPSE_LIFETIME_TICKS};
use arpg_sim::{Accumulator, AttackProfile, Dt, };
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
    Scene { bodies: vec![Placed { pos: (0.0, 3.0), what: ActorDefinition::passive_hostile() }], ..Scene::default() }.into()
}

#[test]
fn full_budget_skips_deaths_and_expiry_reopens_admission() {
    let mut game = Game::empty();
    for batch in 0..=MAX_CORPSES / 8 {
        for i in 0..8 { game.place(Vec2::new(0.0, 2.0 + i as f32), ActorDefinition::passive_hostile()).unwrap(); }
        kill_line(&mut game);
        assert_eq!(game.corpses().len(), ((batch + 1) * 8).min(MAX_CORPSES));
    }
    let newest = game.corpses().last().unwrap().remaining_ticks();
    for _ in 0..newest { step(&mut game, false); }
    assert!(game.corpses().is_empty());
    game.place(Vec2::new(0.0, 3.0), ActorDefinition::passive_hostile()).unwrap();
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
    let replacement = game.place(Vec2::new(0.0, 8.0), ActorDefinition::passive_hostile()).unwrap();
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

#[test]
fn remains_follow_character_presentation_independently_of_health_and_mobility() {
    use std::num::NonZeroU8;
    let mut game = Game::empty();
    let health = NonZeroU8::new(1).unwrap();
    let object = game.place(Vec2::new(0.0, 3.0), ActorDefinition {
        physical: arpg_sim::Template::BODY.with_health(health),
        ..ActorDefinition::passive_hostile()
    }).unwrap();
    let character = game.place(Vec2::new(0.0, 5.0), ActorDefinition {
        physical: arpg_sim::Template::BLOCK.character().with_health(health),
        ..ActorDefinition::passive_hostile()
    }).unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    step(&mut game, true);
    assert_eq!(game.defeats().len(), 2);
    assert!(!game.is_alive(object));
    assert!(!game.is_alive(character));
    assert_eq!(game.enemy_count(), 0);
    assert_eq!(game.corpses().len(), 1);
    assert_eq!(game.corpses()[0].id(), character);
    assert_eq!(game.corpses()[0].remaining_ticks(), CORPSE_LIFETIME_TICKS);
    step(&mut game, false);
    assert!(game.defeats().is_empty());
    assert_eq!(game.corpses().len(), 1);
}

#[test]
fn zero_health_keeps_the_player_allocated_and_does_not_create_remains() {
    use arpg_game::{ActionDefinition, ActionTiming, BehaviorNode, SwipeAction};
    use std::num::NonZeroU8;
    let mut game = Game::empty();
    let player = game.player_id();
    let strike = SwipeAction {
        timing: ActionTiming { startup: 1, active: 1, recovery: 1 },
        damage: NonZeroU8::new(5).unwrap(),
        ..Default::default()
    };
    game.place(Vec2::new(0.0, 1.0), ActorDefinition {
        actions: vec![ActionDefinition::Swipe(strike)],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::passive_hostile()
    }).unwrap();
    step(&mut game, false);
    assert_eq!(game.player_health(), 5);
    step(&mut game, false);
    assert_eq!(game.player_health(), 0);
    assert!(game.is_alive(player));
    assert_eq!(game.player_id(), player);
    assert!(game.defeats().is_empty());
    assert!(game.corpses().is_empty());
    for _ in 0..24 { step(&mut game, false); }
    assert_eq!(game.health(player), Some(0));
    assert!(game.is_alive(player));
    assert!(game.corpses().is_empty());
}
