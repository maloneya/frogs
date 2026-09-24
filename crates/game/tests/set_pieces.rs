//! Predictions through Game: offsets, restart, independent instances and refusal.
use arpg_core::{AimDir, DirectionalLight, Intent, MAX_DIRECTIONAL_LIGHTS, MoveDir, Report};
use arpg_game::{
    Game, GameScene, LightPulse, SetPieceAction, SetPieceCue, SetPieceEvent, SetPieceSpec,
    SetPieceState,
};
use arpg_sim::{Accumulator, Dt, Placed, Template};
use glam::{Vec2, Vec3};

fn cue(at: u32, duration: u32, intensity: f32) -> SetPieceCue {
    SetPieceCue::new(
        at,
        SetPieceAction::LightPulse(
            LightPulse::new(
                DirectionalLight::try_new(Vec3::Y, Vec3::ONE, intensity, 0.1).unwrap(),
                duration,
            )
            .unwrap(),
        ),
    )
    .unwrap()
}
fn piece(name: &str, cues: Vec<SetPieceCue>) -> SetPieceSpec {
    SetPieceSpec::new(name.into(), cues).unwrap()
}
fn scene(cues: Vec<SetPieceCue>) -> GameScene {
    GameScene {
        set_pieces: vec![piece("reveal", cues)],
        ..GameScene::default()
    }
}
fn step(game: &mut Game) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}
fn intensities(game: &Game) -> Vec<f32> {
    game.directional_lights()
        .map(|light| light.intensity())
        .collect()
}
fn observed(game: &Game) -> (u64, String, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish(), game.render_trace_since(0))
}

#[test]
fn delayed_and_overlapping_cues_follow_the_timeline_without_touching_physics() {
    // age 0: no light; 1: first pulse 4; 2: first 2 + second 3;
    // 3: second 1.5; 4: finished. Both cues start in authored order.
    let mut game = Game::from_scene(&scene(vec![cue(1, 2, 4.0), cue(2, 2, 3.0)])).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    let mut control = Game::from_scene(&GameScene::default()).unwrap();
    assert!(intensities(&game).is_empty());
    assert!(game.trigger_set_piece(id, "reveal"));
    assert_eq!(
        game.set_piece_state(id, "reveal"),
        Some(SetPieceState::Queued)
    );
    assert!(
        intensities(&game).is_empty(),
        "commands land on the next tick"
    );
    for expected in [vec![], vec![4.0], vec![2.0, 3.0], vec![1.5], vec![]] {
        step(&mut game);
        step(&mut control);
        assert_eq!(intensities(&game), expected);
        assert_eq!(game.engine_hash(), control.engine_hash());
    }
    assert_eq!(
        game.set_piece_state(id, "reveal"),
        Some(SetPieceState::Inactive)
    );
    assert_eq!(
        game.set_piece_trace().since(0).collect::<Vec<_>>(),
        vec![
            (
                0,
                SetPieceEvent::Started {
                    scene: id,
                    index: 0
                }
            ),
            (
                4,
                SetPieceEvent::Finished {
                    scene: id,
                    index: 0
                }
            ),
        ]
    );
    let hash = game.hash();
    game.clear_trace();
    assert_eq!(game.hash(), hash, "diagnostics are not authoritative");
    assert_eq!(game.set_piece_trace().since(0).count(), 0);
}

#[test]
fn retrigger_and_scene_lifetime_use_stable_identity_and_cached_content() {
    let mut authored = scene(vec![cue(0, 2, 4.0)]);
    let mut game = Game::from_scene(&authored).unwrap();
    let fresh = observed(&game);
    let first = game.scene_instances().next().unwrap().0;
    let second = game.load_scene(&authored).unwrap();
    assert!(game.trigger_set_piece(first, "reveal"));
    step(&mut game);
    assert_eq!(
        game.set_piece_state(second, "reveal"),
        Some(SetPieceState::Inactive)
    );
    assert!(game.trigger_set_piece(second, "reveal"));
    step(&mut game);
    assert_eq!(intensities(&game), vec![2.0, 4.0]);
    assert!(game.trigger_set_piece(first, "reveal"));
    step(&mut game);
    assert_eq!(intensities(&game), vec![4.0, 2.0]);
    assert!(game.evict_scene(first));
    assert_eq!(intensities(&game), vec![2.0]);
    let third = game.load_scene(&authored).unwrap();
    let before = observed(&game);
    assert!(!game.trigger_set_piece(first, "reveal"));
    assert!(!game.trigger_set_piece(second, "missing"));
    assert_eq!(observed(&game), before);
    assert!(game.trigger_set_piece(third, "reveal"));
    assert!(game.evict_scene(third));
    step(&mut game);
    assert!(
        intensities(&game).is_empty(),
        "eviction cancels queued work"
    );
    authored.set_pieces.clear();
    game.restart().unwrap();
    assert_eq!(observed(&game), fresh);
    let id = game.scene_instances().next().unwrap().0;
    assert!(game.trigger_set_piece(id, "reveal"));
    step(&mut game);
    game.start_scene(&GameScene::default()).unwrap();
    assert_eq!(game.set_pieces().count(), 0);
    assert!(intensities(&game).is_empty());
}

#[test]
fn refusal_preserves_active_timelines_pending_spawns_and_restart() {
    let authored = scene(vec![cue(0, 3, 2.0); MAX_DIRECTIONAL_LIGHTS]);
    let mut game = Game::from_scene(&authored).unwrap();
    let mut control = Game::from_scene(&authored).unwrap();
    for game in [&mut game, &mut control] {
        let id = game.scene_instances().next().unwrap().0;
        assert!(game.trigger_set_piece(id, "reveal"));
        step(game);
        assert!(game.request_spawn(Vec2::new(30.0, 0.0), Template::BODY).is_some());
    }
    let before = observed(&game);
    assert!(game.load_scene(&scene(vec![cue(0, 1, 1.0)])).is_err());
    let invalid = GameScene {
        set_pieces: vec![
            piece("duplicate", vec![cue(0, 1, 1.0)]),
            piece("duplicate", vec![cue(0, 1, 1.0)]),
        ],
        ..GameScene::default()
    };
    assert!(game.start_scene(&invalid).is_err());
    assert_eq!(observed(&game), before);
    step(&mut game);
    step(&mut control);
    assert_eq!(observed(&game), observed(&control));
    assert_eq!(game.enemy_count(), 1);
    game.restart().unwrap();
    assert_eq!(
        observed(&game),
        observed(&Game::from_scene(&authored).unwrap())
    );

    let mut empty = Game::empty();
    let before = observed(&empty);
    assert!(empty.load_scene(&invalid).is_err());
    let mut invalid = scene(vec![cue(0, 1, 1.0)]);
    invalid.engine.bodies.push(Placed {
        pos: (f32::NAN, 0.0),
        what: Template::BODY,
    });
    assert!(empty.load_scene(&invalid).is_err());
    assert_eq!(
        observed(&empty),
        before,
        "validate the complete load before mutation"
    );
}

#[test]
fn authored_names_offsets_and_light_settings_affect_live_and_restart_hashes() {
    let descriptions = [
        scene(vec![cue(0, 2, 4.0)]),
        scene(vec![cue(1, 2, 4.0)]),
        scene(vec![cue(0, 3, 4.0)]),
        scene(vec![cue(0, 2, 3.0)]),
        GameScene {
            set_pieces: vec![piece("other", vec![cue(0, 2, 4.0)])],
            ..GameScene::default()
        },
    ];
    let mut hashes = Vec::new();
    let mut restart_hashes = Vec::new();
    for scene in descriptions {
        let mut game = Game::from_scene(&scene).unwrap();
        let hash = game.hash();
        assert!(!hashes.contains(&hash));
        hashes.push(hash);
        let id = game.scene_instances().next().unwrap().0;
        assert!(game.evict_scene(id));
        let hash = game.hash();
        assert!(!restart_hashes.contains(&hash));
        restart_hashes.push(hash);
    }
}

#[test]
fn constructors_refuse_unbounded_or_unreachable_timelines() {
    let light = DirectionalLight::try_new(Vec3::Y, Vec3::ONE, 1.0, 0.0).unwrap();
    assert!(LightPulse::new(light, 0).is_err());
    assert!(LightPulse::new(light, u32::MAX).is_err());
    let action = SetPieceAction::LightPulse(LightPulse::new(light, 1).unwrap());
    assert!(SetPieceCue::new(u32::MAX, action).is_err());
    assert!(SetPieceSpec::new("empty".into(), vec![]).is_err());
    assert!(SetPieceSpec::new("bad name".into(), vec![cue(0, 1, 1.0)]).is_err());
    assert!(SetPieceSpec::new("reverse".into(), vec![cue(1, 1, 1.0), cue(0, 1, 1.0)]).is_err());
    assert!(
        SetPieceSpec::new(
            "excess".into(),
            vec![cue(0, 1, 1.0); MAX_DIRECTIONAL_LIGHTS + 1]
        )
        .is_err()
    );
}
