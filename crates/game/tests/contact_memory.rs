//! Warm-start contact forces belong to live physical contacts, not actors or recycled slots.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{
    ActionDefinition, ActorDefinition, BehaviorNode, Destination, Game, GameScene, MoveAction,
    Placed,
};
use arpg_sim::{Accumulator, Dt, EntityId, Template, TuningField, Wall};
use glam::Vec2;

fn mover(destination: f32) -> ActorDefinition {
    ActorDefinition {
        actions: vec![ActionDefinition::Move(MoveAction {
            destination: Destination::Point((destination, 10.0)),
            speed: 2.0,
            acceleration: 30.0,
            stop_distance: 0.0,
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    }
}
fn idle() -> ActorDefinition {
    ActorDefinition {
        actions: vec![],
        behavior: BehaviorNode::Idle,
        ..mover(10.0)
    }
}
fn scene() -> GameScene {
    let mut setup = GameScene::default();
    setup.engine.bodies = vec![
        Placed {
            pos: (-0.25, 10.0),
            what: mover(10.0),
        },
        Placed {
            pos: (0.25, 10.0),
            what: mover(-10.0),
        },
    ];
    setup
}
fn bodies(game: &Game) -> [EntityId; 2] {
    let owner = game.scene_instances().next().unwrap().0;
    game.scene_bodies(owner).unwrap().try_into().unwrap()
}
fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}
fn observed(game: &Game) -> (u64, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish())
}
fn memory(game: &Game) -> String {
    let (_, report) = observed(game);
    let value = report
        .split_once("\"contact_forces\":")
        .expect("reported contact forces")
        .1;
    // This object contains identity keys and numbers, so brace matching suffices.
    let mut depth = 0;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return value[..=index].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unterminated contact force report");
}
fn pair_key(mut ids: [EntityId; 2]) -> String {
    ids.sort_by_key(|id| id.diagnostic_identity());
    format!("\"{}/{}\":", ids[0], ids[1])
}
fn gap(game: &Game, ids: [EntityId; 2]) -> f32 {
    game.body_pos(ids[1]).unwrap().x - game.body_pos(ids[0]).unwrap().x
}
fn force(game: &Game, ids: [EntityId; 2]) -> f32 {
    let memory = memory(game);
    let record = memory.split_once(&pair_key(ids)).expect("pair force").1;
    record
        .split_once("\"force\":")
        .unwrap()
        .1
        .split([',', '}'])
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
fn kinetic(game: &Game, ids: [EntityId; 2]) -> f32 {
    ids.into_iter()
        .map(|id| 0.5 * game.motion(id).unwrap().velocity().length_squared())
        .sum()
}

#[test]
fn opposed_motors_hold_their_load_and_release_it_without_gaining_energy() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let mut replay = Game::from_scene(&scene()).unwrap();
    let ids = bodies(&game);
    assert_eq!(memory(&game), "{}");
    for _ in 0..120 {
        step(&mut game);
        step(&mut replay);
        assert_eq!(game.hash(), replay.hash());
        assert!(game.all_positions_finite());
    }
    // Each blocked 1kg motor adds 0.5m/s per tick: 30N, carried by the contact.
    // A light load is held rigidly inside the half-millimetre contact slop.
    let load = force(&game, ids);
    assert!(
        (load - 30.0).abs() < 0.05,
        "contact force {load}N, expected 30N"
    );
    assert!(gap(&game, ids) >= 0.5 - 0.0005 - 1e-6);
    for world in [&mut game, &mut replay] {
        for id in ids {
            world.assign_behavior(id, idle()).unwrap();
        }
    }
    // Unforced, overlap correction separates the pair but relaxation keeps
    // the push from turning into velocity.
    let mut previous = kinetic(&game, ids);
    for _ in 0..120 {
        step(&mut game);
        step(&mut replay);
        assert_eq!(game.hash(), replay.hash());
        let current = kinetic(&game, ids);
        assert!(
            current <= previous + 1e-6,
            "unforced energy grew: {previous} -> {current}"
        );
        previous = current;
    }
    assert!(
        gap(&game, ids) >= 0.5 - 0.0005 - 1e-6,
        "overlap relaxes to the slop"
    );
    assert_eq!(memory(&game), "{}", "unloaded contacts keep no force");
}

#[test]
fn rejected_changes_preserve_accepted_forces_and_pending_state() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let ids = bodies(&game);
    for _ in 0..10 {
        step(&mut game);
    }
    assert!(memory(&game).contains(&pair_key(ids)));
    let before = observed(&game);
    let mut invalid_scene = scene();
    invalid_scene.engine.bodies[0].pos.0 = f32::NAN;
    assert!(game.load_scene(&invalid_scene).is_err());
    assert_eq!(observed(&game), before);
    let mut invalid = mover(10.0);
    invalid.physical = Template::BLOCK;
    assert!(game.assign_behavior(ids[0], invalid).is_err());
    assert_eq!(observed(&game), before);
    let mut invalid = mover(10.0);
    let ActionDefinition::Move(settings) = &mut invalid.actions[0] else {
        unreachable!()
    };
    settings.acceleration = f32::NAN;
    assert!(game.assign_behavior(ids[0], invalid).is_err());
    assert_eq!(observed(&game), before);
    for field in [
        TuningField::ContactFrequency,
        TuningField::ContactDamping,
        TuningField::ContactPushSpeed,
    ] {
        assert!(game.tuning().with(field, f32::NAN).is_err());
        assert_eq!(observed(&game), before);
    }
}

#[test]
fn removal_recycled_identity_scene_eviction_and_restart_retire_forces() {
    let setup = scene();
    let mut game = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let [old, other] = bodies(&game);
    for _ in 0..10 {
        step(&mut game);
    }
    assert!(memory(&game).contains(&pair_key([old, other])));
    assert!(game.despawn_body(old));
    assert_eq!(memory(&game), "{}");
    let fresh = game.place(Vec2::new(-10.0, 10.0), idle()).unwrap();
    assert_eq!(old.diagnostic_identity().0, fresh.diagnostic_identity().0);
    assert_ne!(old, fresh);
    game.assign_behavior(other, idle()).unwrap();
    let position = game.body_pos(other).unwrap();
    let velocity = game.motion(other).unwrap().velocity();
    step(&mut game);
    // No remembered force from the removed partner kicks the survivor.
    assert!((game.body_pos(other).unwrap().x - (position.x + velocity.x * Dt::SECS)).abs() < 1e-6);
    assert!((game.motion(other).unwrap().velocity().x - velocity.x * 0.9).abs() < 1e-6);
    assert_eq!(game.motion(fresh).unwrap().velocity(), Vec2::ZERO);
    assert_eq!(memory(&game), "{}");
    assert!(game.evict_scene(owner));
    assert!(game.motion(other).is_none());
    assert!(game.motion(fresh).is_some());
    assert_eq!(memory(&game), "{}");
    game.restart().unwrap();
    assert_eq!(
        observed(&game),
        observed(&Game::from_scene(&setup).unwrap())
    );
}

#[test]
fn evicting_a_wall_discards_its_force_without_kicking_the_surviving_body() {
    let mut setup = GameScene::default();
    setup
        .engine
        .walls
        .push(Wall::new(Vec2::new(0.0, 10.0), Vec2::new(0.2, 4.0), 0.0).unwrap());
    let mut game = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let body = game.place(Vec2::new(0.4, 10.0), mover(-10.0)).unwrap();
    for _ in 0..10 {
        step(&mut game);
    }
    assert!(memory(&game).contains(&format!("{body}/{owner}/wall0")));
    assert!(
        game.body_pos(body).unwrap().x >= 0.35 - 0.001,
        "the wall holds the body"
    );
    game.assign_behavior(body, idle()).unwrap();
    assert!(game.evict_scene(owner));
    assert_eq!(memory(&game), "{}");
    let position = game.body_pos(body).unwrap();
    let velocity = game.motion(body).unwrap().velocity();
    step(&mut game);
    assert!((game.body_pos(body).unwrap().x - (position.x + velocity.x * Dt::SECS)).abs() < 1e-6);
    assert!((game.motion(body).unwrap().velocity().x - velocity.x * 0.9).abs() < 1e-6);
    assert_eq!(memory(&game), "{}");
}

#[test]
fn material_edits_preserve_current_state_and_change_the_next_response() {
    // Material governs overlap correction, so start the pair 10cm deep.
    let mut deep = scene();
    deep.engine.bodies[0].pos.0 = -0.2;
    deep.engine.bodies[1].pos.0 = 0.2;
    for (field, value) in [
        (TuningField::ContactFrequency, 5.0),
        (TuningField::ContactDamping, 0.0),
        (TuningField::ContactPushSpeed, 0.2),
    ] {
        let mut game = Game::from_scene(&deep).unwrap();
        let mut unchanged = Game::from_scene(&deep).unwrap();
        let ids = bodies(&game);
        for world in [&mut game, &mut unchanged] {
            step(world);
        }
        let positions = ids.map(|id| game.body_pos(id).unwrap());
        let velocities = ids.map(|id| game.motion(id).unwrap().velocity());
        let forces = memory(&game);
        game.set_tuning(game.tuning().with(field, value).unwrap());
        assert_eq!(ids.map(|id| game.body_pos(id).unwrap()), positions);
        assert_eq!(
            ids.map(|id| game.motion(id).unwrap().velocity()),
            velocities
        );
        assert_eq!(memory(&game), forces);
        step(&mut game);
        step(&mut unchanged);
        assert_ne!(
            ids.map(|id| game.body_pos(id).unwrap()),
            ids.map(|id| unchanged.body_pos(id).unwrap()),
            "{field:?} changes the next physical response"
        );
    }
}
