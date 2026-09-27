//! Runtime settings compose with movement, captured attacks, identity and restart.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{ActorDefinition, ActionDefinition, Game, GameScene};
use arpg_sim::{Accumulator, Dt, Event, Tuning, TuningField, AttackProfile};
use glam::Vec2;

fn step(game: &mut Game, attack: bool) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, attack));
}

#[test]
fn every_attribute_changes_hash_and_report_and_invalid_edits_preserve_pending_work() {
    for field in TuningField::ALL {
        let mut game = Game::empty();
        let before = game.hash();
        let value = field.attribute().default + 0.5;
        game.set_tuning(game.tuning().with(field, value).unwrap());
        assert_ne!(game.hash(), before, "{field:?}");
        let mut out = Report::default();
        game.report(&mut out);
        assert!(out.finish().contains(&format!("\"{}\":{value}", field.attribute().key)));
        assert!(game.trace().iter().any(|(tick, event)| tick == 0 &&
            event == Event::TuningChanged { field, value }));
        let ticket = game.request_spawn(Vec2::new(20.0, 0.0), ActorDefinition::passive_hostile()).unwrap();
        let before = (game.hash(), game.trace().render());
        for invalid in [f32::NAN, f32::INFINITY, -1.0, field.attribute().max + 1.0] {
            assert!(game.tuning().with(field, invalid).is_err());
            assert_eq!((game.hash(), game.trace().render()), before);
        }
        step(&mut game, false);
        assert_eq!(game.spawn_outcomes()[0].ticket, ticket);
        assert!(game.spawn_outcomes()[0].entity.is_some());
    }
}

#[test]
fn tuning_survives_scene_start_restart_eviction_and_refused_load() {
    let scene = GameScene::default();
    let mut game = Game::from_scene(&scene).unwrap();
    let tuning = Tuning::default().with(TuningField::PlayerSpeed, 3.0).unwrap()
        .with(TuningField::SlamImpulse, 12.0).unwrap();
    game.set_tuning(tuning);
    let owner = game.scene_instances().next().unwrap().0;
    assert!(game.evict_scene(owner));
    assert_eq!(game.tuning(), tuning);
    game.restart().unwrap();
    let initial = game.hash();
    step(&mut game, false);
    game.restart().unwrap();
    assert_eq!(game.hash(), initial);
    game.start_scene(&scene).unwrap();
    assert_eq!(game.hash(), initial);
    let mut bad = GameScene::default();
    bad.engine.bodies.push(arpg_game::Placed { pos: (f32::NAN, 0.0), what: ActorDefinition::passive_hostile() });
    assert!(game.start_scene(&bad).is_err());
    assert_eq!(game.hash(), initial);
    assert_eq!(game.tuning(), tuning);
}

#[test]
fn existing_and_new_monsters_use_new_speed_through_the_normal_motor() {
    let mut game = Game::empty();
    let retired = game.place(Vec2::new(-30.0, 0.0), ActorDefinition::passive_hostile()).unwrap();
    let existing = game.place(Vec2::new(20.0, 0.0), ActorDefinition::hostile()).unwrap();
    for _ in 0..10 { step(&mut game, false); }
    assert!(game.despawn_body(retired)); // moves the existing monster's dense row
    let mut slow = ActorDefinition::hostile();
    let ActionDefinition::Move(movement) = &mut slow.actions[0] else { panic!("movement preset") };
    movement.speed = 1.0;
    game.assign_behavior(existing, slow.clone()).unwrap();
    let new = game.place(Vec2::new(-20.0, 0.0), slow).unwrap();
    step(&mut game, false);
    assert!((game.motion(new).unwrap().velocity().x - 0.5).abs() < 1e-6, "accelerates .5 m/s on first tick");
    for _ in 0..10 { step(&mut game, false); }
    assert_eq!(game.motion(existing).unwrap().velocity(), Vec2::new(-1.0, 0.0));
    assert_eq!(game.motion(new).unwrap().velocity(), Vec2::new(1.0, 0.0));
    assert!(game.motion(retired).is_none());
}

#[test]
fn impulse_edit_during_a_swing_changes_only_the_next_swing_for_both_profiles() {
    for (profile, field, position) in [
        (AttackProfile::Cleave, TuningField::CleaveImpulse, Vec2::new(-2.8, 1.2)),
        (AttackProfile::Slam, TuningField::SlamImpulse, Vec2::new(0.0, 1.5)),
    ] {
        let mut game = Game::empty();
        let target = game.place(position, ActorDefinition::passive_hostile()).unwrap();
        game.set_attack_profile(profile);
        let original = profile.resolve(game.tuning()).unwrap();
        step(&mut game, true);
        game.set_tuning(game.tuning().with(field, 12.0).unwrap());
        for _ in 0..original.startup() { step(&mut game, false); }
        let speed = game.motion(target).unwrap().velocity().length();
        assert!((speed - original.knockback()).abs() < 1e-5, "in-flight impulse: {profile:?} {speed}");
        assert!(game.despawn_body(target));
        let duration = original.startup() + original.active() + original.recovery().get();
        while game.tick() < u64::from(duration) { step(&mut game, false); }
        let next = game.place(position, ActorDefinition::passive_hostile()).unwrap();
        step(&mut game, true);
        for _ in 0..original.startup() { step(&mut game, false); }
        assert!((game.motion(next).unwrap().velocity().length() - 12.0).abs() < 1e-5);
    }
}

#[test]
fn increased_motor_acceleration_allows_speeds_above_the_old_drag_ceiling() {
    let mut game = Game::empty();
    let mut fast = ActorDefinition::hostile();
    let ActionDefinition::Move(movement) = &mut fast.actions[0] else { panic!("movement preset") };
    movement.speed = 10.0;
    movement.acceleration = 300.0;
    let id = game.place(Vec2::new(40.0, 0.0), fast).unwrap();
    // Acceleration of 300m/s² adds up to 5m/s each tick: 5, 9.5, then 10.
    for speed in [5.0, 9.5, 10.0] {
        step(&mut game, false);
        assert!((game.motion(id).unwrap().velocity().x + speed).abs() < 1e-5);
    }
}
