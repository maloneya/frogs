//! Scene lighting admission, lifetime and deterministic observations through Game.
use arpg_core::{AimDir, Intent, LinearRgb, MAX_POINT_LIGHTS, MoveDir, PointLight, Report};
use arpg_game::{Game, GameScene, TorchSpec};
use arpg_sim::{Accumulator, Dt, Placed, Template};
use glam::{Vec2, Vec3};

fn torch(x: f32) -> TorchSpec {
    TorchSpec::new(PointLight::new(
        Vec3::new(x, 2.2, 1.5),
        LinearRgb::new(1.0, 0.52, 0.18),
        24.0,
        14.0,
    ))
    .unwrap()
}
fn scene(count: usize, x: f32) -> GameScene {
    GameScene {
        torches: (0..count).map(|i| torch(x + i as f32)).collect(),
        ..GameScene::default()
    }
}
fn observed(game: &Game) -> (u64, String, String) {
    let mut out = Report::default();
    game.report(&mut out);
    (game.hash(), out.finish(), game.render_trace_since(0))
}
fn step(game: &mut Game) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}

#[test]
fn torches_follow_scene_identity_eviction_and_cached_restart() {
    let mut authored = scene(2, 0.0);
    let mut game = Game::from_scene(&authored).unwrap();
    let original = game.scene_instances().next().unwrap().0;
    let fresh = observed(&game);
    let second = game.load_scene(&scene(3, 10.0)).unwrap();
    assert_eq!(game.torch_count(), 5);
    assert_eq!(game.scene_torch(second, 2), Some(torch(12.0)));
    assert!(game.evict_scene(original));
    assert_eq!(game.torch_count(), 3);
    assert!(
        observed(&game).1.contains(&format!("\"{second}/0\"")),
        "report identity survives earlier scene eviction"
    );
    assert_eq!(game.scene_torch(original, 0), None);
    let third = game.load_scene(&scene(5, 20.0)).unwrap();
    assert_eq!(game.torch_count(), MAX_POINT_LIGHTS);
    assert!(
        !game.evict_scene(original),
        "stale scene cannot evict reused storage"
    );
    assert_eq!(game.scene_torch(second, 0), Some(torch(10.0)));
    assert_eq!(game.scene_torch(third, 4), Some(torch(24.0)));
    assert_eq!(
        game.torches()
            .map(|t| t.light().position().x)
            .collect::<Vec<_>>(),
        vec![10.0, 11.0, 12.0, 20.0, 21.0, 22.0, 23.0, 24.0]
    );
    authored.torches.clear();
    game.restart().unwrap();
    assert_eq!(
        observed(&game),
        fresh,
        "restart restores cached torches, IDs, and reports"
    );
    game.start_scene(&GameScene::default()).unwrap();
    assert_eq!(
        game.torch_count(),
        0,
        "an unlit scene inherits no global torch"
    );
}

#[test]
fn refused_lighting_loads_preserve_live_state_snapshot_and_pending_work() {
    let authored = scene(MAX_POINT_LIGHTS, 0.0);
    let mut game = Game::from_scene(&authored).unwrap();
    let mut control = Game::from_scene(&authored).unwrap();
    for game in [&mut game, &mut control] {
        assert!(game.request_spawn(Vec2::new(40.0, 0.0), Template::BODY).is_some());
    }
    let before = observed(&game);
    assert!(game.load_scene(&scene(1, 50.0)).is_err());
    assert!(
        game.start_scene(&scene(MAX_POINT_LIGHTS + 1, 50.0))
            .is_err()
    );
    assert_eq!(observed(&game), before);
    step(&mut game);
    step(&mut control);
    assert_eq!(observed(&game), observed(&control));
    assert_eq!(
        game.enemy_count(),
        1,
        "accepted queued work survived refusal"
    );
    game.restart().unwrap();
    assert_eq!(
        observed(&game),
        observed(&Game::from_scene(&authored).unwrap())
    );

    // Even with room for its light, invalid physical content must install nothing.
    let mut empty = Game::empty();
    let before = observed(&empty);
    let mut invalid = scene(1, 0.0);
    invalid.engine.bodies.push(Placed {
        pos: (f32::NAN, 0.0),
        what: Template::BODY,
    });
    assert!(empty.load_scene(&invalid).is_err());
    assert_eq!(observed(&empty), before);
}

#[test]
fn authored_light_changes_affect_complete_hash_but_not_physics() {
    let first = scene(1, 0.0);
    let mut changed = first.clone();
    changed.torches[0] = torch(1.0);
    let mut a = Game::from_scene(&first).unwrap();
    let mut b = Game::from_scene(&changed).unwrap();
    assert_eq!(a.engine_hash(), b.engine_hash());
    assert_ne!(a.hash(), b.hash());
    // The cached restart effect remains distinguishable after live eviction.
    let id_a = a.scene_instances().next().unwrap().0;
    let id_b = b.scene_instances().next().unwrap().0;
    assert!(a.evict_scene(id_a));
    assert!(b.evict_scene(id_b));
    assert_ne!(a.hash(), b.hash());
}

#[test]
fn environment_belongs_to_scenes_and_survives_torch_changes_and_restart() {
    use arpg_core::EnvironmentLight;
    use arpg_game::EnvironmentSpec;
    let dark = EnvironmentLight::try_new(LinearRgb::WHITE, 0.0, LinearRgb::WHITE, 0.0, Vec3::Y).unwrap();
    let blue = EnvironmentLight::try_new(LinearRgb::new(0.0, 0.0, 1.0), 0.15, LinearRgb::WHITE, 0.1, Vec3::new(-1.0, 2.0, 0.0)).unwrap();
    let mut authored = scene(1, 0.0);
    authored.environment = EnvironmentSpec::new(dark);
    let mut game = Game::from_scene(&authored).unwrap();
    let first = game.scene_instances().next().unwrap().0;
    let fresh = observed(&game);
    assert_eq!(game.environment_light(), dark);
    step(&mut game);
    assert_eq!(game.environment_light(), dark, "ticks never change scene illumination");
    // A newly loaded scene selects its explicit environment even without lights.
    let next = GameScene { environment: EnvironmentSpec::new(blue), ..GameScene::default() };
    let second = game.load_scene(&next).unwrap();
    assert_eq!(game.torch_count(), 1);
    assert_eq!(game.environment_light(), blue);
    assert!(observed(&game).1.contains(&format!("\"scene\":\"{second}\"")));
    let before = observed(&game);
    let mut invalid = scene(MAX_POINT_LIGHTS, 10.0);
    invalid.environment = EnvironmentSpec::default();
    assert!(game.load_scene(&invalid).is_err());
    assert_eq!(observed(&game), before, "failed admission cannot select an environment");
    assert!(game.evict_scene(second));
    assert_eq!(game.environment_light(), dark, "eviction reveals the prior live environment");
    assert!(!game.evict_scene(second));
    assert_eq!(game.environment_light(), dark, "stale scene identities have no effect");
    assert!(game.evict_scene(first));
    assert_eq!(game.environment_light(), EnvironmentLight::default());
    game.restart().unwrap();
    assert_eq!(observed(&game), fresh, "restart restores complete authored lighting");
    let newer = game.load_scene(&next).unwrap();
    let older = game.scene_instances().next().unwrap().0;
    assert!(game.evict_scene(older));
    assert_eq!(game.torch_count(), 0);
    assert_eq!(game.environment_light(), blue, "removing the final torch cannot change the newer environment");
    assert!(game.evict_scene(newer));
}

#[test]
fn every_environment_component_affects_game_hash_without_changing_physics() {
    use arpg_core::EnvironmentLight;
    use arpg_game::EnvironmentSpec;
    let original = Game::from_scene(&GameScene::default()).unwrap();
    for light in [
        EnvironmentLight::try_new(LinearRgb::new(1.0, 0.0, 0.0), 0.22, LinearRgb::WHITE, 0.35, Vec3::new(0.4, 1.0, 0.3)).unwrap(),
        EnvironmentLight::try_new(LinearRgb::WHITE, 0.4, LinearRgb::WHITE, 0.35, Vec3::new(0.4, 1.0, 0.3)).unwrap(),
        EnvironmentLight::try_new(LinearRgb::WHITE, 0.22, LinearRgb::new(0.0, 0.0, 1.0), 0.35, Vec3::new(0.4, 1.0, 0.3)).unwrap(),
        EnvironmentLight::try_new(LinearRgb::WHITE, 0.22, LinearRgb::WHITE, 0.5, Vec3::new(0.4, 1.0, 0.3)).unwrap(),
        EnvironmentLight::try_new(LinearRgb::WHITE, 0.22, LinearRgb::WHITE, 0.35, Vec3::Y).unwrap(),
    ] {
        let authored = GameScene { environment: EnvironmentSpec::new(light), ..GameScene::default() };
        let mut changed = Game::from_scene(&authored).unwrap();
        assert_eq!(changed.engine_hash(), original.engine_hash());
        assert_ne!(changed.hash(), original.hash());
        let id = changed.scene_instances().next().unwrap().0;
        assert!(changed.evict_scene(id));
        changed.restart().unwrap();
        assert_eq!(changed.environment_light(), light);
    }
}
