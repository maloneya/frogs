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
// Admission and lifetime fixtures remain still; moving deaths are tested below.
fn stationary_enemy() -> ActorDefinition {
    ActorDefinition {
        physical: arpg_sim::Template::BLOCK.character().with_health(std::num::NonZeroU8::new(3).unwrap()),
        ..ActorDefinition::passive_hostile()
    }
}
fn scene() -> GameScene {
    Scene { bodies: vec![Placed { pos: (0.0, 3.0), what: stationary_enemy() }], ..Scene::default() }.into()
}

#[test]
fn full_budget_skips_deaths_and_expiry_reopens_admission() {
    let mut game = Game::empty();
    for batch in 0..=MAX_CORPSES / 8 {
        for i in 0..8 { game.place(Vec2::new(0.0, 2.0 + i as f32), stationary_enemy()).unwrap(); }
        kill_line(&mut game);
        assert_eq!(game.corpses().len(), ((batch + 1) * 8).min(MAX_CORPSES));
    }
    let newest = game.corpses().last().unwrap().remaining_ticks();
    for _ in 0..newest { step(&mut game, false); }
    assert!(game.corpses().is_empty());
    game.place(Vec2::new(0.0, 3.0), stationary_enemy()).unwrap();
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

fn lethal_scene(wall: bool) -> GameScene {
    Scene {
        bodies: vec![Placed {
            pos: (-2.8, 1.2),
            what: ActorDefinition {
                physical: arpg_sim::Template::BODY.character().with_health(std::num::NonZeroU8::MIN),
                ..ActorDefinition::passive_hostile()
            },
        }],
        walls: if wall {
            vec![arpg_sim::Wall::new(Vec2::new(-2.8, 1.8), Vec2::new(4.0, 0.2), 0.0).unwrap()]
        } else { vec![] },
        ..Scene::default()
    }.into()
}
fn lethal_cleave(game: &mut Game) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, true));
    for _ in 0..6 { step(game, false); }
}

#[test]
fn corpse_is_admitted_at_death_and_follows_motion_without_restarting_its_age() {
    let scene = lethal_scene(false);
    let mut game = Game::from_scene(&scene).unwrap();
    let mut replay = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let body = game.scene_bodies(owner).unwrap()[0];
    lethal_cleave(&mut game);
    lethal_cleave(&mut replay);
    assert_eq!(game.enemy_count(), 0);
    assert_eq!(game.health(body), Some(0));
    assert!(game.action_status(body).is_none());
    assert_eq!(game.defeats().len(), 1);
    assert_eq!(game.corpses().len(), 1);
    assert_eq!(game.body_pos(body).unwrap().z, 1.2);
    assert_eq!(game.motion(body).unwrap().velocity(), Vec2::new(0.0, 6.0));
    assert!(!game.character_presentations(arpg_sim::Alpha::ONE).any(|row| row.id() == body));
    assert_eq!(game.hash(), replay.hash());
    step(&mut game, false);
    step(&mut replay, false);
    assert!(game.defeats().is_empty(), "death is published only once");
    assert_eq!(game.corpses().len(), 1);
    assert!((game.body_pos(body).unwrap().z - 1.3).abs() < 0.00001);
    let mut elapsed = 1;
    for _ in 0..100 {
        if !game.is_alive(body) { break; }
        assert_eq!(game.corpses().len(), 1);
        assert_eq!(game.hash(), replay.hash());
        step(&mut game, false);
        step(&mut replay, false);
        elapsed += 1;
        assert!(game.defeats().is_empty());
    }
    assert!(!game.is_alive(body));
    assert!(!game.despawn_body(body));
    assert!(!game.apply_impulse(body, arpg_sim::Impulse::try_from((1.0, 0.0)).unwrap()));
    let corpse = game.corpses()[0];
    assert_eq!(corpse.id(), body);
    assert!((corpse.position().y - 2.2).abs() < 0.0002, "free knockback carries the full impulse / 6 metres");
    assert_eq!(corpse.remaining_ticks(), CORPSE_LIFETIME_TICKS - elapsed);
    assert_eq!(game.hash(), replay.hash());
    step(&mut game, false);
    assert_eq!(game.corpses()[0].position(), corpse.position());
    assert_eq!(game.corpses()[0].remaining_ticks(), CORPSE_LIFETIME_TICKS - elapsed - 1);
}

#[test]
fn killing_impacts_still_meet_walls_and_transfer_momentum() {
    for wall in [false, true] {
        let mut game = Game::from_scene(&lethal_scene(wall)).unwrap();
        let owner = game.scene_instances().next().unwrap().0;
        let body = game.scene_bodies(owner).unwrap()[0];
        let tuning = game.tuning().with(arpg_sim::TuningField::CleaveImpulse, 72.0).unwrap();
        game.set_tuning(tuning);
        lethal_cleave(&mut game);
        let neighbor = (!wall).then(|| game.place(Vec2::new(-2.8, 1.8), ActorDefinition::passive_hostile()).unwrap());
        assert_eq!(game.health(body), Some(0));
        for _ in 0..150 {
            if !game.is_alive(body) { break; }
            step(&mut game, false);
        }
        assert!(!game.is_alive(body));
        if wall {
            let z = game.corpses()[0].position().y;
            assert!((1.44..=1.451).contains(&z), "wall stops the body at its radius: {z}");
        } else {
            let neighbor = neighbor.unwrap();
            assert!(game.body_pos(neighbor).unwrap().z > 1.9, "the defeated body pushes its neighbor");
            assert_eq!(game.health(neighbor), Some(3));
        }
    }
}

#[test]
fn pending_death_obeys_refusal_eviction_and_restart() {
    let scene = lethal_scene(false);
    let mut game = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let body = game.scene_bodies(owner).unwrap()[0];
    lethal_cleave(&mut game);
    let before = game.hash();
    let mut invalid = scene.clone();
    invalid.engine.bodies[0].pos.0 = f32::NAN;
    assert!(game.load_scene(&invalid).is_err());
    assert!(game.start_scene(&invalid).is_err());
    assert_eq!(game.hash(), before);
    let mut report = Report::default();
    game.report(&mut report);
    assert!(report.finish().contains(&format!("\"defeat_announced\":{{\"{}\":false,\"{body}\":true", game.player_id())));
    assert!(game.evict_scene(owner));
    assert!(!game.is_alive(body));
    let replacement = game.place(Vec2::new(20.0, 0.0), ActorDefinition::passive_hostile()).unwrap();
    assert_ne!(body, replacement);
    for _ in 0..100 { step(&mut game, false); }
    assert!(game.corpses().is_empty(), "eviction never manufactures remains");
    game.restart().unwrap();
    lethal_cleave(&mut game);
    game.restart().unwrap();
    assert_eq!(game.hash(), Game::from_scene(&scene).unwrap().hash());
    assert!(game.corpses().is_empty());
}

#[test]
fn lethal_slam_carries_the_same_momentum_as_a_surviving_hit_until_rest() {
    let mut content = Scene {
        bodies: vec![Placed {
            pos: (0.0, 1.0),
            what: ActorDefinition::passive_hostile(),
        }],
        ..Scene::default()
    };
    let mut survivor = Game::from_scene(&content.clone().into()).unwrap();
    content.bodies[0].what.physical = arpg_sim::Template::BODY.character()
        .with_health(std::num::NonZeroU8::MIN);
    let mut defeated = Game::from_scene(&content.into()).unwrap();
    let body = defeated.scene_bodies(defeated.scene_instances().next().unwrap().0).unwrap()[0];
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    for game in [&mut survivor, &mut defeated] {
        game.set_attack_profile(AttackProfile::Slam);
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, true));
        for _ in 0..9 { step(game, false); }
    }
    assert_eq!(survivor.health(body), Some(1));
    assert_eq!(defeated.health(body), Some(0));
    assert_eq!(defeated.enemy_count(), 0);
    assert_eq!(defeated.defeats().len(), 1);
    assert_eq!(defeated.motion(body).unwrap().velocity(), Vec2::new(0.0, 72.0));
    for elapsed in 1..=120 {
        step(&mut survivor, false);
        step(&mut defeated, false);
        assert!(defeated.defeats().is_empty(), "death counts only once");
        let resting = survivor.motion(body).unwrap().velocity() == Vec2::ZERO;
        if resting {
            assert!(!defeated.is_alive(body));
            let corpse = defeated.corpses()[0];
            assert!((corpse.position().y - survivor.body_pos(body).unwrap().z).abs() < 0.00001);
            assert!((corpse.position().y - 13.0).abs() < 0.0002, "72 kg m/s travels about 12m before rest");
            assert_eq!(corpse.remaining_ticks(), CORPSE_LIFETIME_TICKS - elapsed);
            return;
        }
        assert_eq!(defeated.corpses().len(), 1);
        assert_eq!(defeated.corpses()[0].remaining_ticks(), CORPSE_LIFETIME_TICKS - elapsed);
        assert_eq!(defeated.corpses()[0].position().y, defeated.body_pos(body).unwrap().z);
        assert_eq!(defeated.body_pos(body), survivor.body_pos(body));
        assert_eq!(defeated.motion(body).unwrap().velocity(), survivor.motion(body).unwrap().velocity());
    }
    panic!("free knockback must settle within 120 ticks");
}

#[test]
fn death_expiration_bounds_a_body_kept_moving_and_admin_removal_clears_its_record() {
    let mut game = Game::from_scene(&lethal_scene(false)).unwrap();
    let body = game.scene_bodies(game.scene_instances().next().unwrap().0).unwrap()[0];
    lethal_cleave(&mut game);
    assert_eq!(game.corpses()[0].remaining_ticks(), CORPSE_LIFETIME_TICKS);
    for age in 1..=CORPSE_LIFETIME_TICKS {
        let push = if age % 2 == 0 { 6.0 } else { -6.0 };
        assert!(game.apply_impulse(body, arpg_sim::Impulse::try_from((push, 0.0)).unwrap()));
        step(&mut game, false);
        if age < CORPSE_LIFETIME_TICKS {
            assert!(game.is_alive(body));
            assert_eq!(game.corpses()[0].remaining_ticks(), CORPSE_LIFETIME_TICKS - age);
        }
    }
    assert!(!game.is_alive(body));
    assert!(game.corpses().is_empty());
    game.restart().unwrap();
    let body = game.scene_bodies(game.scene_instances().next().unwrap().0).unwrap()[0];
    lethal_cleave(&mut game);
    assert!(game.despawn_body(body));
    assert!(game.corpses().is_empty());
    assert!(!game.despawn_body(body));
}
