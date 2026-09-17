#[test]
fn f3_toggles_collision_drawing_without_simulation_input() {
    let mut app = App {
        game: Game::empty(),
        ..Default::default()
    };
    let before = app.game.hash();
    app.on_debug_key(KeyCode::F3);
    assert!(app.collision_debug.enabled());
    assert_eq!(app.collision_debug.drawings().len(), 1);
    app.on_debug_key(KeyCode::F3);
    assert!(!app.collision_debug.enabled());
    assert!(app.collision_debug.drawings().is_empty());
    assert_eq!(app.game.hash(), before);
}

use super::*;
use crate::harness::Command;

#[test]
fn a_capture_without_a_renderer_is_rejected_immediately() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App {
        harness: Some(rx),
        ..Default::default()
    };
    let (reply, response) = std::sync::mpsc::channel();
    tx.send(Request {
        command: Command::Shot("unused.png".into()),
        reply,
    })
    .unwrap();
    assert!(!app.drain_harness());
    assert_eq!(response.try_recv().unwrap(), "error: no renderer yet");
    assert!(app.capture.path().is_none());
}

#[test]
fn deferred_replies_wait_for_release_and_preserve_future_deadlines() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App {
        harness: Some(rx),
        ..Default::default()
    };
    let (hold_reply, held) = std::sync::mpsc::channel();
    let (wait_reply, waited) = std::sync::mpsc::channel();
    tx.send(Request {
        command: Command::Hold(KeyCode::KeyD, 0),
        reply: hold_reply,
    })
    .unwrap();
    tx.send(Request {
        command: Command::Wait(60_000),
        reply: wait_reply,
    })
    .unwrap();
    assert!(!app.drain_harness());
    assert_ne!(app.input.held().move_axis(), Vec2::ZERO);
    assert!(
        held.try_recv().is_err(),
        "hold replies only after its release"
    );
    app.service_schedule();
    assert_eq!(held.try_recv().unwrap(), "ok");
    assert_eq!(app.input.held().move_axis(), Vec2::ZERO);
    assert_eq!(app.scheduled.len(), 1);
    assert!(waited.try_recv().is_err());
    app.scheduled[0].due = std::time::Instant::now();
    app.service_schedule();
    assert_eq!(waited.try_recv().unwrap(), "ok");
    assert!(app.scheduled.is_empty());
}

fn pair() -> Scene {
    arpg_sim::Scene {
        name: "pair".into(),
        bodies: vec![arpg_sim::Placed {
            pos: (20.0, 0.0),
            what: arpg_sim::Template::BODY,
        }],
        grids: vec![],
        sources: vec![],
    }
    .into()
}

fn tap(app: &mut App, key: KeyCode) {
    app.handle_key(key, true, false);
    app.handle_key(key, false, false);
}

#[test]
fn picker_uses_the_fresh_start_boundary_and_survives_bad_files() {
    let directory = std::env::temp_dir().join(format!("arpg-picker-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("directory.ron")).unwrap();
    let path = directory.join("a scene.ron");
    std::fs::write(&path, "(name: \"first\", bodies: [(pos: (20.0, 0.0))])").unwrap();
    std::fs::write(directory.join("b.ron"), "broken RON").unwrap();
    std::fs::write(directory.join("ignored.txt"), "ignored").unwrap();
    let paths = scene_catalog(&directory).unwrap();
    assert_eq!(paths, vec![path.clone(), directory.join("b.ron")]);
    assert!(scene_catalog(&directory.join("missing")).is_err());

    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    tap(&mut app, KeyCode::F2);
    app.input.menu_mut().set_catalog(Ok(paths.clone()));
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::Enter);
    let initial = app.game.hash();
    let direct = Game::from_scene(&arpg_content::load_scene(&path).unwrap()).unwrap();
    assert_eq!(
        initial,
        direct.hash(),
        "picker and file loader produce identical tick-zero worlds"
    );
    assert_eq!(app.run_id, 2);
    assert!(!app.input.menu().open());
    assert_eq!(app.input.sample().move_axis(), Vec2::ZERO);
    std::fs::write(&path, "(name: \"changed\")").unwrap();
    tap(&mut app, KeyCode::F2);
    tap(&mut app, KeyCode::Enter); // Cached restart remains independent of disk.
    assert_eq!(app.game.hash(), initial);
    assert_eq!(app.run_id, 3);

    tap(&mut app, KeyCode::F2);
    app.input.menu_mut().set_catalog(Ok(paths.clone()));
    for _ in 0..3 {
        tap(&mut app, KeyCode::KeyS);
    }
    tap(&mut app, KeyCode::Enter);
    assert!(app.input.menu().picker().error().unwrap().contains("b.ron"));
    assert!(app.input.menu().open());
    assert_eq!(app.game.hash(), initial);
    assert_eq!(app.run_id, 3);
    std::fs::remove_file(&path).unwrap();
    tap(&mut app, KeyCode::KeyW);
    tap(&mut app, KeyCode::Enter); // A file disappearing after discovery is safe too.
    assert!(app.input.menu().picker().error().is_some());
    assert_eq!(app.game.hash(), initial);

    std::fs::write(&path, "(name: \"changed\")").unwrap();
    tap(&mut app, KeyCode::Enter); // Retry reads the repaired file.
    assert_ne!(app.game.hash(), initial);
    assert_eq!(app.run_id, 4);
    assert!(!app.input.menu().open());
    tap(&mut app, KeyCode::F2);
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(
        app.game.hash(),
        Game::from_scene(&Scene::boot()).unwrap().hash()
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn exhausted_run_ids_preserve_the_game_input_and_pending_capture() {
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    app.run_id = u64::MAX;
    let profile = app.game.attack_status().profile;
    app.input.on_key(KeyCode::KeyW, true, false, profile);
    let (reply, response) = std::sync::mpsc::channel();
    app.capture
        .request("pending.png".into(), Some(reply), std::time::Instant::now());
    let before = app.game.hash();
    let held = app.input.held().move_axis();

    assert!(app.start_playtest(Scene::boot()).is_err());
    assert!(app.restart_playtest().is_err());
    assert_eq!(app.game.hash(), before);
    assert_eq!(app.game.selected_scene_name(), Some("pair"));
    assert_eq!(app.run_id, u64::MAX);
    assert_eq!(app.input.held().move_axis(), held);
    assert!(app.capture.path().is_some());
    assert!(matches!(
        response.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn restart_resets_the_complete_playtest_boundary() {
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    let initial = app.game.hash();
    let profile = app.game.attack_status().profile;
    app.input.on_key(KeyCode::KeyW, true, false, profile);
    app.input.on_key(KeyCode::Space, true, false, profile);
    for dt in app.accumulator.pending(arpg_sim::Dt::SECS) {
        app.game
            .step(dt, Intent::new(MoveDir::new(glam::Vec3::X), true));
    }
    for dt in app.accumulator.pending(arpg_sim::Dt::SECS * 8.0) {
        app.game.step(dt, Intent::NONE);
        app.attack_effects.observe(&app.game);
    }
    app.attack_effects.rebuild(app.game.tick(), Alpha::ZERO);
    assert!(!app.attack_effects.vertices().is_empty());
    app.game
        .set_attack_recovery(arpg_sim::RecoveryTicks::try_from(1).unwrap());
    assert!(app.game.apply_impulse(
        app.game.player_id(),
        arpg_sim::Impulse::try_from((6.0, 0.0)).unwrap()
    ));
    assert!(app.game.request_spawn(Vec2::ZERO, arpg_sim::Template::BODY));
    app.input.on_key(KeyCode::F1, true, false, profile);
    app.input.on_key(KeyCode::ArrowRight, true, false, profile);
    app.input.on_key(KeyCode::Enter, true, false, profile);
    assert!(app.input.menu().pending().is_some());
    let mut camera = OrthoCamera::new(1280, 720);
    camera.snap_to(glam::Vec3::new(40.0, 0.0, 40.0));
    app.camera = Some(camera);
    assert_eq!(app.accumulator.pending(arpg_sim::Dt::SECS * 0.5).count(), 0);

    app.restart_playtest().unwrap();
    assert!(app.attack_effects.vertices().is_empty());
    assert_eq!(app.run_id, 2);
    assert_eq!(
        app.game.hash(),
        initial,
        "all simulation state returns to the baseline"
    );
    assert_eq!(app.game.tick(), 0);
    assert_eq!(app.accumulator.alpha().get(), 0.0);
    assert_eq!(app.camera.as_ref().unwrap().target(), glam::Vec3::ZERO);
    assert!(!app.input.menu().open());
    assert!(app.input.take_profile().is_none());
    assert_eq!(app.input.sample().move_axis(), Vec2::ZERO);
    app.input.on_key(KeyCode::KeyW, true, false, profile);
    assert_eq!(
        app.input.sample().move_axis(),
        Vec2::ZERO,
        "held native keys require release"
    );
    app.input.on_key(KeyCode::KeyW, false, false, profile);
    app.input.on_key(KeyCode::KeyW, true, false, profile);
    assert_ne!(app.input.sample().move_axis(), Vec2::ZERO);
}

#[test]
fn restart_cancels_old_delayed_actions_and_capture_replies() {
    let mut app = App::default();
    let (reply, response) = std::sync::mpsc::channel();
    let (shot_reply, shot_response) = std::sync::mpsc::channel();
    app.input
        .on_key(KeyCode::KeyD, true, false, app.game.attack_status().profile);
    app.scheduled.push(Deferred {
        due: std::time::Instant::now() + std::time::Duration::from_secs(60),
        release: Some(KeyCode::KeyD),
        reply: Some(reply),
    });
    app.capture.request(
        "pending.png".into(),
        Some(shot_reply),
        std::time::Instant::now(),
    );
    app.start_playtest(pair()).unwrap();
    assert!(response.try_recv().unwrap().contains("cancelled"));
    assert!(shot_response.try_recv().unwrap().contains("cancelled"));
    assert!(app.scheduled.is_empty());
    app.input
        .on_key(KeyCode::KeyD, true, false, app.game.attack_status().profile);
    app.service_schedule();
    assert_ne!(
        app.input.sample().move_axis(),
        Vec2::ZERO,
        "new presses survive cancelled old releases"
    );
}

#[test]
fn rejected_replacement_preserves_the_current_playtest() {
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    app.input
        .on_key(KeyCode::KeyW, true, false, app.game.attack_status().profile);
    let before = app.report_state();
    let trace = app.game.trace().render();
    let mut bad = pair();
    bad.engine.bodies[0].pos.0 = f32::INFINITY;
    assert!(app.start_playtest(bad).is_err());
    assert_eq!(app.report_state(), before);
    assert_eq!(app.game.trace().render(), trace);
    assert_ne!(app.input.sample().move_axis(), Vec2::ZERO);
}

#[test]
fn harness_start_reads_content_but_restart_reuses_the_snapshot() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.harness = Some(rx);
    let mut command = |command| {
        let (reply, response) = std::sync::mpsc::channel();
        tx.send(Request { command, reply }).unwrap();
        assert!(!app.drain_harness());
        (response.try_recv().unwrap(), app.game.hash(), app.run_id)
    };
    let path = std::env::temp_dir().join(format!("arpg-scene-snapshot-{}.ron", std::process::id()));
    std::fs::write(&path, "(name: \"first\", bodies: [(pos: (20.0, 0.0))])").unwrap();
    let (reply, first, _) = command(Command::StartScene(path.clone()));
    assert!(reply.contains("ready run=1 tick=0"), "{reply}");
    std::fs::write(&path, "(name: \"second\")").unwrap();
    let (_, restarted, run) = command(Command::RestartScene);
    assert_eq!(first, restarted);
    assert_eq!(run, 2);
    let (_, reloaded, run) = command(Command::StartScene(path.clone()));
    assert_ne!(first, reloaded);
    assert_eq!(run, 3);
    std::fs::remove_file(&path).unwrap();
    let (reply, unchanged, run) = command(Command::StartScene(path));
    assert!(reply.starts_with("error:"));
    assert_eq!(unchanged, reloaded);
    assert_eq!(run, 3);
}
