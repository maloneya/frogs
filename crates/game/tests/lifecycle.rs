//! Complete game lifecycle through the public boundary used by the app.

use arpg_core::{Intent, MoveDir, Report};
use arpg_game::{Game, GameScene, RestartError, SceneError};
use arpg_sim::{
    Accumulator, AttackProfile, BodyGrid, Condition, Dt, Impulse, InteractionState,
    Placed, RecoveryTicks, Scene, SourceId, SourceSpec, SourceState, Template,
};
use glam::{Vec2, Vec3};

fn scene() -> GameScene {
    Scene {
        name: "cached trial".into(),
        bodies: vec![Placed { pos: (1.0, 0.0), what: Template::BLOCK.interactive() }],
        sources: vec![SourceSpec {
            enabled: true,
            pos: (30.0, 0.0), radius: 2.0, every: 2,
            when: Condition::Always, what: Template::BODY,
        }],
        grids: vec![],
    }.into()
}

fn step(game: &mut Game, intent: Intent) {
    for dt in Accumulator::default().pending(Dt::SECS) {
        game.step(dt, intent);
    }
}

fn report(game: &Game) -> String {
    let mut out = Report::default();
    game.report(&mut out);
    out.finish()
}

#[test]
fn failed_start_and_additive_load_preserve_the_complete_run() {
    let selected = scene();
    let mut game = Game::from_scene(&selected).unwrap();
    let mut control = Game::from_scene(&selected).unwrap();
    for run in [&mut game, &mut control] {
        step(run, Intent::NONE.with_interact(true));
        assert!(run.request_spawn(Vec2::new(-20.0, 0.0), Template::BODY));
        run.set_attack_profile(AttackProfile::Sweep);
    }
    let before = (game.hash(), report(&game), game.trace().render());
    let mut invalid = scene();
    invalid.engine.name = "must not become the restart snapshot".into();
    invalid.engine.bodies.push(Placed { pos: (f32::NAN, 0.0), what: Template::BODY });
    let mut oversized = scene();
    oversized.engine.grids.push(BodyGrid {
        origin: (0.0, 4.0), columns: usize::MAX, rows: 2,
        spacing: 1.0, what: Template::BODY,
    });

    for rejected in [&invalid, &oversized] {
        assert!(game.start_scene(rejected).is_err());
        assert!(game.load_scene(rejected).is_err());
        assert_eq!((game.hash(), report(&game), game.trace().render()), before);
        assert_eq!(game.selected_scene_name(), Some(selected.engine.name.as_str()));
    }
    assert_eq!(game.start_scene(&oversized), Err(SceneError::Engine(arpg_sim::SceneError::Capacity)));
    step(&mut game, Intent::NONE);
    step(&mut control, Intent::NONE);
    assert_eq!(game.hash(), control.hash(), "accepted pending work must survive refusal");
    assert_eq!(game.enemy_count(), 2, "first emission plus the accepted queued body");
    game.restart().unwrap();
    assert_eq!(game.hash(), Game::from_scene(&selected).unwrap().hash());
}

#[test]
fn restart_uses_its_snapshot_and_replaces_all_live_state_and_pending_work() {
    let mut authored = scene();
    let mut game = Game::from_scene(&authored).unwrap();
    let mut fresh = Game::from_scene(&authored).unwrap();
    let original = game.scene_instances().next().unwrap().0;
    let prop = game.scene_bodies(original).unwrap()[0];
    step(&mut game, Intent::NONE.with_interact(true));
    assert_eq!(game.interaction_state(prop), Some(InteractionState::Activated));
    let extra = game.load_scene(&Scene {
        name: "additive".into(),
        bodies: vec![Placed { pos: (-40.0, 0.0), what: Template::BODY }],
        ..Scene::default()
    }.into()).unwrap();
    game.set_attack_profile(AttackProfile::Sweep);
    game.set_attack_recovery(RecoveryTicks::try_from(1).unwrap());
    assert!(game.apply_impulse(game.player_id(), Impulse::try_from((6.0, 0.0)).unwrap()));
    step(&mut game, Intent::new(MoveDir::new(Vec3::Z), true));
    assert!(game.request_spawn(Vec2::new(-50.0, 0.0), Template::BODY));
    assert!(game.evict_scene(original));
    assert!(game.scene_bodies(extra).is_some());
    // Changing caller-owned content and evicting its live instance must not
    // change what restart reconstructs. No file or second cleanup call is used.
    authored.engine.name = "edited after load".into();
    authored.engine.sources.clear();
    game.restart().unwrap();
    assert_eq!(game.tick(), 0);
    assert_eq!(game.hash(), fresh.hash());
    assert_eq!(report(&game), report(&fresh));
    assert_eq!(game.trace().render(), fresh.trace().render());
    assert_eq!(game.scene_count(), 1);
    assert_eq!(game.source_count(), 1);
    assert_eq!(game.selected_scene_name(), Some("cached trial"));
    // Identities are scoped to a run; inspect new membership rather than
    // interpreting old ids against a replacement world.
    let restored = game.scene_instances().next().unwrap().0;
    let restored_prop = game.scene_bodies(restored).unwrap()[0];
    assert_eq!(game.interaction_state(restored_prop), Some(InteractionState::Ready));
    for _ in 0..4 {
        step(&mut game, Intent::NONE);
        step(&mut fresh, Intent::NONE);
        assert_eq!(game.hash(), fresh.hash(), "old queued work must not reappear");
    }
    assert_eq!(game.enemy_count(), 2, "only fresh source emissions at ticks 0 and 2");
}

#[test]
fn eviction_is_local_and_preserves_the_restart_choice() {
    let selected = scene();
    let mut game = Game::from_scene(&selected).unwrap();
    let a = game.scene_instances().next().unwrap().0;
    let b = game.load_scene(&selected).unwrap();
    step(&mut game, Intent::NONE);
    let retired = game.scene_bodies(a).unwrap().to_vec();
    let survivors = game.scene_bodies(b).unwrap().to_vec();
    assert_eq!(retired.len(), 2, "one authored prop and one emitted body");
    assert!(game.request_spawn(Vec2::new(-40.0, 0.0), Template::BODY));
    assert!(game.evict_scene(a));
    assert!(!game.evict_scene(a));
    assert!(retired.iter().all(|id| !game.is_alive(*id)));
    assert!(survivors.iter().all(|id| game.is_alive(*id)));
    assert_eq!(game.selected_scene_name(), Some(selected.engine.name.as_str()));
    for _ in 0..4 {
        step(&mut game, Intent::NONE);
    }
    assert_eq!(game.enemy_count(), 4, "B's three emissions plus unowned queued work");
    assert_eq!(game.source_count(), 1);
    let c = game.load_scene(&selected).unwrap();
    assert_ne!(a, c);
    assert!(game.evict_scene(c)); // Eviction before the first source evaluation.
    step(&mut game, Intent::NONE); // Tick 5: B is waiting; C must not emit.
    assert_eq!(game.enemy_count(), 4);
}

#[test]
fn restart_choice_is_hashed_even_when_active_worlds_are_identical() {
    let content = scene();
    let cached = Game::from_scene(&content).unwrap();
    let mut additive = Game::empty();
    additive.load_scene(&content).unwrap();
    assert_eq!(cached.engine_hash(), additive.engine_hash());
    assert_ne!(cached.hash(), additive.hash(), "restart is a different future input");
    let before = (additive.hash(), report(&additive), additive.trace().render());
    assert_eq!(additive.restart(), Err(RestartError::NoScene));
    assert_eq!((additive.hash(), report(&additive), additive.trace().render()), before);

    // Different cached descriptions must remain distinguishable even after
    // both of their live instances have been removed.
    let first_scene = scene();
    let mut second_scene = scene();
    second_scene.engine.sources[0].every = 7;
    let mut first = Game::from_scene(&first_scene).unwrap();
    let mut second = Game::from_scene(&second_scene).unwrap();
    let id = first.scene_instances().next().unwrap().0;
    assert!(first.evict_scene(id));
    let id = second.scene_instances().next().unwrap().0;
    assert!(second.evict_scene(id));
    assert_eq!(first.engine_hash(), second.engine_hash());
    assert_eq!(first.selected_scene_name(), second.selected_scene_name());
    assert_ne!(first.hash(), second.hash());
}

#[test]
fn source_enablement_is_instance_local_and_restart_restores_authored_state() {
    let mut authored = scene();
    authored.engine.sources[0].enabled = false;
    let mut game = Game::from_scene(&authored).unwrap();
    let fresh_hash = game.hash();
    let original = game.scene_instances().next().unwrap().0;
    let extra = game.load_scene(&authored).unwrap();
    // Fresh runs allocate sources in installation order. These names are
    // scoped to this run; restart assertions below compare the complete hash.
    let a = SourceId::parse("s0").unwrap();
    let b = SourceId::parse("s1").unwrap();
    let dormant = Some(SourceState { enabled: false, countdown: 0, emitted: 0 });
    step(&mut game, Intent::NONE);
    assert_eq!(game.source_state(a), dormant);
    assert_eq!(game.source_state(b), dormant);
    assert!(game.set_source_enabled(a, true));
    step(&mut game, Intent::NONE);
    assert_eq!(game.enemy_count(), 1);
    assert_eq!(game.source_state(b), dormant);
    assert!(game.evict_scene(original));
    assert!(!game.set_source_enabled(a, true));
    assert_eq!(game.source_state(a), None);
    assert!(game.scene_bodies(extra).is_some());
    assert_eq!(game.source_state(b), dormant);
    assert_eq!(game.enemy_count(), 0);
    assert!(game.set_source_enabled(b, true));
    step(&mut game, Intent::NONE);
    assert_eq!(game.enemy_count(), 1);
    game.restart().unwrap();
    assert_eq!(game.hash(), fresh_hash);
    step(&mut game, Intent::NONE);
    assert_eq!(game.enemy_count(), 0, "restart must restore authored dormancy");
}
