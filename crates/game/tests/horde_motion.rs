//! Authored movement choices, bounded motors and stable identity through Game.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{ActionDefinition, ActionKind, ActorDefinition, Game, Placed, Scene};
use arpg_sim::{Accumulator, Dt};
use glam::Vec2;
fn step(game: &mut Game) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(), Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}
fn moving(speed:f32) -> ActorDefinition {
    let mut definition=ActorDefinition::hostile();
    let ActionDefinition::Move(movement)=&mut definition.actions[0] else { panic!("move preset") };
    movement.speed=speed; definition
}
fn scene() -> arpg_game::GameScene {
    Scene { bodies:vec![Placed{pos:(20.0,0.0),what:moving(2.5)},Placed{pos:(-20.0,0.0),what:moving(4.0)},Placed{pos:(0.0,20.0),what:moving(1.5)}],..Default::default() }.into()
}
#[test]
fn changing_an_authored_action_accelerates_instead_of_replacing_velocity() {
    let mut game=Game::from_scene(&scene()).unwrap();
    let owner=game.scene_instances().next().unwrap().0;
    let ids=game.scene_bodies(owner).unwrap().to_vec();
    for _ in 0..27 {step(&mut game);}
    for (id,speed) in ids.iter().zip([2.5,4.0,1.5]) {assert!((game.motion(*id).unwrap().velocity().length()-speed).abs()<1e-5);}
    game.assign_behavior(ids[0],moving(4.0)).unwrap();
    game.assign_behavior(ids[1],moving(2.5)).unwrap();
    step(&mut game);
    assert!((game.motion(ids[0]).unwrap().velocity().x+2.75).abs()<1e-5);
    assert!((game.motion(ids[1]).unwrap().velocity().x-3.1).abs()<1e-5);
    assert_eq!(game.action_status(ids[0]).unwrap().kind,ActionKind::Move);
    let mut report=Report::default();game.report(&mut report);
    assert!(report.finish().contains("actions"));
    let before=game.hash();game.clear_trace();assert_eq!(game.hash(),before);
}
#[test]
fn row_moves_stale_ids_eviction_and_restart_preserve_independent_execution() {
    let mut game=Game::from_scene(&scene()).unwrap();
    let mut control=Game::from_scene(&scene()).unwrap();
    let owner=game.scene_instances().next().unwrap().0;
    let ids=game.scene_bodies(owner).unwrap().to_vec();
    assert!(game.despawn_body(ids[0]));
    let replacement=game.place(Vec2::new(0.0,-30.0),ActorDefinition::passive_hostile()).unwrap();
    let before=game.hash();
    assert!(game.assign_behavior(ids[0],moving(2.5)).is_err());
    assert_eq!(game.hash(),before);
    assert_eq!(game.action_status(replacement).unwrap().kind,ActionKind::Idle);
    assert!(game.assign_behavior(game.player_id(),moving(2.5)).is_err());
    let block=game.place(Vec2::new(30.0,30.0),ActorDefinition::prop()).unwrap();
    assert!(game.assign_behavior(block,moving(2.5)).is_err());
    for _ in 0..90 {step(&mut game);step(&mut control);for &id in &ids[1..]{assert_eq!(game.body_pos(id),control.body_pos(id));assert_eq!(game.motion(id).unwrap().velocity(),control.motion(id).unwrap().velocity());}}
    assert!(game.evict_scene(owner));for id in ids{assert!(game.action_status(id).is_none());}
    game.restart().unwrap();let mut fresh=Game::from_scene(&scene()).unwrap();
    assert_eq!(game.hash(),fresh.hash());
    for _ in 0..220{step(&mut game);step(&mut fresh);assert_eq!(game.hash(),fresh.hash());}
}
