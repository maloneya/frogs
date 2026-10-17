#[test]
fn companion_toggles_collision_drawing_without_simulation_input() {
    let mut app = App {
        game: Game::empty(),
        ..Default::default()
    };
    let before = app.game.hash();
    assert_eq!(companion(&mut app, serde_json::json!({"op":"collision", "enabled":true}))["ok"], true);
    assert!(app.collision_debug.enabled());
    assert_eq!(app.collision_debug.drawings().len(), 1);
    assert_eq!(companion(&mut app, serde_json::json!({"op":"collision", "enabled":false}))["ok"], true);
    assert!(!app.collision_debug.enabled());
    assert!(app.collision_debug.drawings().is_empty());
    assert_eq!(app.game.hash(), before);
}

use super::*;
use crate::harness::Command;

#[test]
fn maximum_admitted_lights_survive_weather_and_flicker_without_changing_game() {
    use glam::Vec3;
    let environment = arpg_core::EnvironmentLight::try_new(
        arpg_core::LinearRgb::WHITE, 1_000_000.0,
        arpg_core::LinearRgb::WHITE, 1_000_000.0, Vec3::Y,
    ).unwrap();
    let torch = arpg_game::TorchSpec::new(arpg_core::PointLight::new(
        Vec3::new(0.0, 1.4, 0.0), arpg_core::LinearRgb::WHITE, 1_000_000.0, 11.0,
    )).unwrap();
    let game = Game::from_scene(&arpg_game::GameScene {
        environment: arpg_game::EnvironmentSpec::new(environment).with_storm(true),
        torches: vec![torch],
        ..Default::default()
    }).unwrap();
    let hash = game.hash();
    let mut flames = crate::torch::Torches::default();
    for seconds in [0.08, 4.0, 4.22] {
        let lit = crate::atmosphere::illumination(game.environment_light(), game.storm(), seconds);
        assert_eq!(lit.ambient_intensity(), environment.ambient_intensity());
        assert_eq!(lit.directional_intensity(), environment.directional_intensity());
        flames.rebuild(game.torches(), &[], seconds);
        assert!(flames.lights()[0].intensity() <= torch.light().intensity());
        assert_eq!(game.hash(), hash);
    }
}

#[test]
fn weather_report_tracks_rebuilt_geometry_without_affecting_gameplay() {
    let mut app = App::default();
    let camera = OrthoCamera::new(1280, 720);
    let before = app.game.hash();
    for enabled in [true, false] {
        app.atmosphere.rebuild(enabled, 2.0, &camera, &[]);
        let state: serde_json::Value = serde_json::from_str(&app.report_state()).unwrap();
        assert_eq!(state["render"]["weather_vertices"], app.atmosphere.vertices().len());
        assert_eq!(state["render"]["weather_draws"], usize::from(enabled));
        assert_eq!(app.game.hash(), before);
    }
}

#[test]
fn harness_audio_toggle_survives_scene_changes_without_touching_gameplay() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App { harness: Some(rx), ..Default::default() };
    for enabled in [false, true] {
        let before = app.game.hash();
        let (reply, response) = std::sync::mpsc::channel();
        tx.send(Request { command: Command::SetAudioEnabled(enabled), reply }).unwrap();
        assert!(!app.drain_harness());
        assert_eq!(response.try_recv().unwrap(), if enabled { "audio on" } else { "audio off" });
        assert_eq!(app.game.hash(), before);
        app.start_playtest(pair()).unwrap();
        app.restart_playtest().unwrap();
        let state: serde_json::Value = serde_json::from_str(&app.report_state()).unwrap();
        assert_eq!(state["audio"]["muted"], !enabled);
    }
}

#[test]
fn invalid_harness_sources_preserve_pending_work() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App { game: Game::empty(), harness: Some(rx), ..Default::default() };
    let ticket = app.game.request_spawn(Vec2::new(20.0, 0.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    let before = (app.game.hash(), app.game.trace().render());
    for input in [
        "source (pos: (0.0, 0.0), radius: -1.0)",
        "source (pos: (0.0, 0.0), when: PlayerWithin(-3.0))",
        "source (pos: (NaN, 0.0))",
    ] {
        let (reply, response) = std::sync::mpsc::channel();
        let spec = arpg_content::parse_source(input.strip_prefix("source ").unwrap()).unwrap();
        tx.send(Request { command: Command::Source(spec), reply }).unwrap();
        assert!(!app.drain_harness());
        assert!(response.try_recv().unwrap().starts_with("error:"));
        assert_eq!((app.game.hash(), app.game.trace().render()), before);
    }
    let (reply, response) = std::sync::mpsc::channel();
    let invalid = arpg_game::ActorDefinition {
        actions: vec![arpg_game::ActionDefinition::Wait { ticks: 0 }],
        behavior: arpg_game::BehaviorNode::Action(0),
        ..arpg_game::ActorDefinition::civilian()
    };
    tx.send(Request { command: Command::Spawn { x: 4.0, z: 4.0, what: invalid }, reply }).unwrap();
    assert!(!app.drain_harness());
    assert_eq!(response.try_recv().unwrap(), "error: invalid wait duration");
    assert_eq!((app.game.hash(), app.game.trace().render()), before);
    for dt in arpg_sim::Accumulator::default().pending(arpg_sim::Dt::SECS) {
        app.game.step(dt, arpg_core::Intent::new(arpg_core::MoveDir::NONE, arpg_core::AimDir::FORWARD, false));
    }
    assert_eq!(app.game.source_count(), 0);
    assert_eq!(app.game.enemy_count(), 1);
    assert_eq!(app.game.spawn_outcomes()[0].ticket, ticket);
    assert!(app.game.spawn_outcomes()[0].entity.is_some());
}

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
    arpg_game::Scene {
        walls: Vec::new(),
        name: "pair".into(),
        bodies: vec![arpg_game::Placed {
            pos: (20.0, 0.0),
            what: arpg_game::ActorDefinition::passive_hostile(),
        }],
        grids: vec![],
        sources: vec![],
    }
    .into()
}

fn companion(app: &mut App, mut command: serde_json::Value) -> serde_json::Value {
    command["session"] = serde_json::json!(std::process::id());
    serde_json::from_str(&app.companion_reply(serde_json::from_value(command).unwrap())).unwrap()
}

#[test]
fn companion_uses_the_fresh_start_boundary_and_survives_bad_files() {
    use serde_json::json;
    let directory = std::env::temp_dir().join(format!("arpg-companion-scenes-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("a scene.ron");
    std::fs::write(&path, "(scene: (engine: (name: \"first\", bodies: [(pos: (20.0, 0.0))])))").unwrap();
    std::fs::write(directory.join("b.ron"), "broken RON").unwrap();
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    app.input.on_key(KeyCode::KeyW, true, false);
    let response = companion(&mut app, json!({"op":"start_scene", "path":path}));
    assert_eq!(response["ok"], true);
    assert_eq!(response["state"]["scenes"]["current"], "first");
    let initial = app.game.hash();
    assert_eq!(initial, Game::from_scene(&arpg_content::load_scene(&path).unwrap()).unwrap().hash());
    assert_eq!(app.run_id, 2);
    assert_eq!(app.input.sample().move_axis(), Vec2::ZERO);
    std::fs::write(&path, "(scene: (engine: (name: \"changed\")))").unwrap();
    assert_eq!(companion(&mut app, json!({"op":"restart"}))["ok"], true);
    assert_eq!(app.game.hash(), initial, "cached restart is independent of disk");
    assert_eq!(app.run_id, 3);

    let failed = companion(&mut app, json!({"op":"start_scene", "path":directory.join("b.ron")}));
    assert_eq!(failed["ok"], false);
    assert!(failed["error"].as_str().unwrap().contains("b.ron"));
    assert_eq!(app.game.hash(), initial);
    assert_eq!(app.run_id, 3);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(companion(&mut app, json!({"op":"start_scene", "path":path}))["ok"], false);
    assert_eq!(app.game.hash(), initial);

    std::fs::write(&path, "(scene: (engine: (name: \"changed\")))").unwrap();
    assert_eq!(companion(&mut app, json!({"op":"start_scene", "path":path}))["ok"], true);
    assert_ne!(app.game.hash(), initial);
    assert_eq!(app.run_id, 4);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn exhausted_run_ids_preserve_the_game_input_and_pending_capture() {
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    app.run_id = u64::MAX;
    app.input.on_key(KeyCode::KeyW, true, false);
    let (reply, response) = std::sync::mpsc::channel();
    app.capture
        .request("pending.png".into(), Some(reply), std::time::Instant::now()).unwrap();
    let before = app.game.hash();
    let held = app.input.held().move_axis();

    assert!(app.start_playtest(Scene::default()).is_err());
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
    app.input.on_key(KeyCode::KeyW, true, false);
    app.input.on_key(KeyCode::Space, true, false);
    for dt in app.accumulator.pending(arpg_sim::Dt::SECS) {
        app.game
            .step(dt, Intent::new(MoveDir::new(glam::Vec3::X), arpg_core::AimDir::new(glam::Vec3::X).unwrap(), true));
    }
    for dt in app.accumulator.pending(arpg_sim::Dt::SECS * 8.0) {
        let intent = Intent::new(arpg_core::MoveDir::NONE, arpg_core::AimDir::FORWARD, false);
        app.game.step(dt, intent);
        app.attack_effects.observe(&app.game, || None);
        app.audio.observe(&app.game, intent);
    }
    let audio_report = |app: &App| {
        let mut out = arpg_core::Report::default();
        app.audio.report(&mut out);
        out.finish()
    };
    assert!(audio_report(&app).contains("\"cleave\":1"));
    let before_audio = audio_report(&app);
    let mut invalid = pair();
    invalid.engine.bodies[0].pos.0 = f32::NAN;
    assert!(app.start_playtest(invalid).is_err());
    assert_eq!(audio_report(&app), before_audio, "rejected replacement preserves audio history");
    app.attack_effects.rebuild(
        &app.game, Alpha::ZERO, Some(crate::attack_effects::BeamEmitter { tip: glam::Vec3::Y * 0.85, direction: glam::Vec3::Z }));
    assert!(!app.attack_effects.vertices().is_empty());
    app.game.set_attack_profile(arpg_game::AttackProfile::Slam);
    assert!(app.game.apply_impulse(
        app.game.player_id(),
        arpg_sim::Impulse::try_from((6.0, 0.0)).unwrap()
    ));
    assert!(app.game.request_spawn(Vec2::ZERO, arpg_game::ActorDefinition::passive_hostile()).is_some());
    let mut camera = OrthoCamera::new(1280, 720);
    camera.snap_to(glam::Vec3::new(40.0, 0.0, 40.0));
    app.camera = Some(camera);
    assert_eq!(app.accumulator.pending(arpg_sim::Dt::SECS * 0.5).count(), 0);

    app.restart_playtest().unwrap();
    assert!(audio_report(&app).contains("\"cleave\":0"));
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
    assert_eq!(app.input.sample().move_axis(), Vec2::ZERO);
    app.input.on_key(KeyCode::KeyW, true, false);
    assert_eq!(
        app.input.sample().move_axis(),
        Vec2::ZERO,
        "held native keys require release"
    );
    app.input.on_key(KeyCode::KeyW, false, false);
    app.input.on_key(KeyCode::KeyW, true, false);
    assert_ne!(app.input.sample().move_axis(), Vec2::ZERO);
}

#[test]
fn restart_cancels_old_delayed_actions_and_capture_replies() {
    let mut app = App::default();
    let (reply, response) = std::sync::mpsc::channel();
    let (shot_reply, shot_response) = std::sync::mpsc::channel();
    app.input
        .on_key(KeyCode::KeyD, true, false);
    app.scheduled.push(Deferred {
        due: std::time::Instant::now() + std::time::Duration::from_secs(60),
        release: Some(KeyCode::KeyD),
        reply: Some(reply),
    });
    app.capture.request(
        "pending.png".into(),
        Some(shot_reply),
        std::time::Instant::now(),
    ).unwrap();
    app.start_playtest(pair()).unwrap();
    assert!(response.try_recv().unwrap().contains("cancelled"));
    assert!(shot_response.try_recv().unwrap().contains("cancelled"));
    assert!(app.scheduled.is_empty());
    app.input
        .on_key(KeyCode::KeyD, true, false);
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
        .on_key(KeyCode::KeyW, true, false);
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
    std::fs::write(&path, "(scene: (engine: (name: \"first\", bodies: [(pos: (20.0, 0.0))])))").unwrap();
    let (reply, first, _) = command(Command::StartScene(path.clone()));
    assert!(reply.contains("ready run=1 tick=0"), "{reply}");
    std::fs::write(&path, "(scene: (engine: (name: \"second\")))").unwrap();
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

/// Synthetic time avoids flaky sleeps while checking the real harness boundary.
fn record_perf_frame(app: &mut App) {
    let start = std::time::Instant::now();
    app.perf.begin_frame(start);
    app.perf.finish_frame(crate::perf::FrameWork {
        ticks: 1, presented: true, ..Default::default()
    });
    app.perf.begin_frame(start + std::time::Duration::from_millis(20));
}

#[test]
fn perf_queries_and_reset_preserve_gameplay_and_pending_work() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App { harness: Some(rx), ..Default::default() };
    app.start_playtest(pair()).unwrap();
    assert!(app.game.request_spawn(Vec2::ZERO, arpg_game::ActorDefinition::passive_hostile()).is_some());
    record_perf_frame(&mut app);
    let before = app.report_state();
    let trace = app.game.trace().render();
    let mut query = |command| {
        let (reply, response) = std::sync::mpsc::channel();
        tx.send(Request { command, reply }).unwrap();
        assert!(!app.drain_harness());
        response.try_recv().unwrap()
    };
    let summary = query(Command::Perf { samples: false });
    assert!(summary.contains("\"samples\":1"));
    assert!(!summary.contains("\"bodies\""));
    assert!(!summary.contains("\"sim_hash\""));
    assert!(!summary.contains("\"frames\""));
    let raw = query(Command::Perf { samples: true });
    assert!(raw.contains("\"frames\":{\"0\":{\"frame_ms\":20.0000"));
    assert_eq!(query(Command::ResetPerf), "ok");
    assert!(query(Command::Perf { samples: false }).contains("\"samples\":0"));
    assert_eq!(app.report_state(), before);
    assert_eq!(app.game.trace().render(), trace);
}

#[test]
fn successful_scene_replacement_resets_perf_but_rejected_content_preserves_it() {
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    record_perf_frame(&mut app);
    let before = app.report_perf(true);
    let mut invalid = pair();
    invalid.engine.bodies[0].pos.0 = f32::INFINITY;
    assert!(app.start_playtest(invalid).is_err());
    assert_eq!(app.report_perf(true), before);
    app.restart_playtest().unwrap();
    assert!(app.report_perf(true).contains("\"samples\":0"));
    record_perf_frame(&mut app);
    app.start_playtest(pair()).unwrap();
    assert!(app.report_perf(true).contains("\"samples\":0"));
}

#[test]
fn performance_trial_refusal_preserves_game_and_pending_work() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App { harness: Some(rx), ..Default::default() };
    let before = app.game.hash();
    let spec = crate::trial::Spec::parse("1 60 idle".split_whitespace()).unwrap();
    let (reply, response) = std::sync::mpsc::channel();
    tx.send(Request { command: Command::PerfTrial(spec), reply }).unwrap();
    app.drain_harness();
    assert_eq!(response.try_recv().unwrap(), "error: no renderer yet");
    assert_eq!(app.game.hash(), before);
    assert!(app.trial.is_none());
    let (reply, response) = std::sync::mpsc::channel();
    tx.send(Request { command: Command::PreparePerf, reply }).unwrap();
    app.drain_harness();
    assert_eq!(response.try_recv().unwrap(), "error: no window yet");
    assert_eq!(app.game.hash(), before);
    assert_eq!(app.performance_readiness(), Err("no window yet"));
    let observation: serde_json::Value = serde_json::from_str(&app.report_perf(false)).unwrap();
    assert_eq!(observation["readiness"]["focused"], false);
    assert_eq!(observation["readiness"]["surface"], false);

    let (reply, pending) = std::sync::mpsc::channel();
    app.trial = Some(crate::trial::Trial::new(spec, reply, app.game.enemy_count()));
    let (reply, response) = std::sync::mpsc::channel();
    tx.send(Request { command: Command::ResetPerf, reply }).unwrap();
    app.drain_harness();
    assert_eq!(response.try_recv().unwrap(), "error: performance trial already running");
    assert!(app.trial.is_some());
    assert!(pending.try_recv().is_err());
    assert_eq!(app.game.hash(), before);
}

#[test]
fn successful_restart_cancels_performance_trial_but_failed_load_preserves_it() {
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    let spec = crate::trial::Spec::parse("1 60 idle".split_whitespace()).unwrap();
    let (reply, response) = std::sync::mpsc::channel();
    app.trial = Some(crate::trial::Trial::new(spec, reply, app.game.enemy_count()));
    assert!(app.start_scene_file(std::path::Path::new("/nonexistent/arpg-perf-scene.ron")).is_err());
    assert!(app.trial.is_some());
    assert!(response.try_recv().is_err());
    app.restart_playtest().unwrap();
    assert!(app.trial.is_none());
    assert_eq!(response.try_recv().unwrap(), "error: cancelled by playtest restart");
}

#[test]
fn contact_history_survives_rejected_loads_and_clears_on_restart_and_eviction() {
    let mut scene = pair();
    scene.engine.bodies[0].pos = (0.0, 4.0);
    let mut app = App::default();
    app.start_playtest(scene.clone()).unwrap();
    let dt = arpg_sim::Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    let hit = |app: &mut App| {
        app.game.set_attack_profile(arpg_game::AttackProfile::Disintegrate);
        app.game.step(dt, Intent::new(MoveDir::NONE, arpg_core::AimDir::FORWARD, false)
            .with_attack_held(true));
        app.enemy_feedback.observe(&app.game, |_, _| {});
        let id = app.game.character_presentations(Alpha::ZERO).next().unwrap().id();
        assert_eq!(app.enemy_feedback.flash(id, app.game.tick(), Alpha::ZERO), 1.0);
        id
    };
    let id = hit(&mut app);
    let mut invalid = scene;
    invalid.engine.bodies[0].pos.0 = f32::NAN;
    assert!(app.start_playtest(invalid).is_err());
    assert_eq!(app.enemy_feedback.flash(id, app.game.tick(), Alpha::ZERO), 1.0);
    app.restart_playtest().unwrap();
    assert_eq!(app.enemy_feedback.flash(id, app.game.tick(), Alpha::ZERO), 0.0);
    let id = hit(&mut app);
    let owner = app.game.scene_instances().next().unwrap().0;
    assert!(app.game.evict_scene(owner));
    app.enemy_feedback.observe(&app.game, |_, _| {});
    assert_eq!(app.enemy_feedback.flash(id, app.game.tick(), Alpha::ZERO), 0.0);
}

#[test]
fn tuning_edits_save_restore_and_restart_through_the_shared_app_boundary() {
    use crate::companion::Edit;
    use arpg_game::{Tuning, TuningField};
    let directory = std::env::temp_dir().join(format!("arpg-tuning-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("tuning.ron");
    let mut app = App { tuning_path: path.clone(), ..Default::default() };
    app.start_playtest(Scene::default()).unwrap();
    let pending = app.game.request_spawn(Vec2::new(20.0, 0.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    let before = app.game.hash();
    assert!(app.edit_tuning(Edit::Set(TuningField::PlayerSpeed, f32::NAN)).is_err());
    assert_eq!(app.game.hash(), before);
    for dt in Accumulator::default().pending(arpg_sim::Dt::SECS) {
        app.game.step(dt, Intent::new(MoveDir::NONE, arpg_core::AimDir::FORWARD, false));
    }
    assert_eq!(app.game.spawn_outcomes()[0].ticket, pending);
    app.edit_tuning(Edit::Set(TuningField::PlayerSpeed, 3.0)).unwrap();
    app.edit_tuning(Edit::Set(TuningField::ContactFrequency, 20.0)).unwrap();
    app.edit_tuning(Edit::Set(TuningField::ContactDamping, 0.75)).unwrap();
    app.edit_tuning(Edit::Set(TuningField::ContactPushSpeed, 1.5)).unwrap();
    app.edit_tuning(Edit::Save).unwrap();
    assert_eq!(arpg_content::load_tuning(&path).unwrap(), app.game.tuning());
    app.edit_tuning(Edit::Defaults).unwrap();
    assert_eq!(app.game.tuning(), Tuning::default());
    app.edit_tuning(Edit::Revert).unwrap();
    app.edit_tuning(Edit::Restart).unwrap();
    assert_eq!(app.game.tuning().get(TuningField::PlayerSpeed), 3.0);
    assert_eq!(app.game.tuning().get(TuningField::ContactFrequency), 20.0);
    assert_eq!(app.game.tuning().get(TuningField::ContactDamping), 0.75);
    assert_eq!(app.game.tuning().get(TuningField::ContactPushSpeed), 1.5);
    app.tuning_path = directory.join("missing-parent").join("tuning.ron");
    let saved = app.tuning_saved;
    app.edit_tuning(Edit::Set(TuningField::PlayerSpeed, 4.0)).unwrap();
    assert!(app.edit_tuning(Edit::Save).is_err());
    assert_eq!(app.tuning_saved, saved);
    assert_eq!(arpg_content::load_tuning(&path).unwrap(), saved);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn companion_catalog_edits_errors_and_harness_changes_share_authority() {
    use crate::companion::{Command, Edit};
    use arpg_game::TuningField;
    let (requests, receiver) = std::sync::mpsc::channel();
    let mut app = App { harness: Some(receiver), ..Default::default() };
    let read = |app: &mut App, command| -> serde_json::Value {
        serde_json::from_str(&app.companion_reply(command)).unwrap()
    };
    let initial = read(&mut app, Command::State {});
    let fields = initial["state"]["fields"].as_array().unwrap();
    assert_eq!(fields.len(), TuningField::ALL.len());
    for (entry, field) in fields.iter().zip(TuningField::ALL) {
        let attribute = field.attribute();
        assert_eq!(entry["key"], attribute.key);
        assert_eq!(entry["group"], attribute.group);
        assert_eq!(entry["unit"], attribute.unit);
        for (name, value) in [("default", attribute.default), ("min", attribute.min),
            ("max", attribute.max), ("step", attribute.step)] {
            assert_eq!(entry[name].as_f64().unwrap() as f32, value);
        }
        assert_eq!(entry["value"].as_f64().unwrap() as f32, app.game.tuning().get(field));
    }
    let before = app.game.hash();
    let stale = read(&mut app, Command::Set { session: 0, key: "player_speed".into(), value: 50.0 });
    assert_eq!(stale["ok"], false);
    assert_eq!(app.game.hash(), before);
    for (key, value) in [("player_speed", -1.0), ("missing", 3.0), ("slam_impulse", f32::INFINITY),
        ("contact_frequency", 0.0), ("contact_damping", 20.5), ("contact_push_speed", 0.0)] {
        let reply = read(&mut app, Command::Set { session: std::process::id(), key: key.into(), value });
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["state"]["error"], true);
        assert_eq!(reply["state"]["message"], reply["error"]);
        assert_eq!(app.game.hash(), before);
    }
    for (field, value) in [(TuningField::ContactFrequency, 20.0),
        (TuningField::ContactDamping, 0.75), (TuningField::ContactPushSpeed, 1.5)] {
        let reply = read(&mut app, Command::Set { session: std::process::id(),
            key: field.attribute().key.into(), value });
        assert_eq!(reply["ok"], true);
        assert_eq!(app.game.tuning().get(field), value);
        let command = crate::harness::Command::TuningEdit(Edit::Set(field, field.attribute().default));
        let (reply, response) = std::sync::mpsc::channel();
        requests.send(crate::harness::Request { command, reply }).unwrap();
        assert!(!app.drain_harness());
        assert!(!response.try_recv().unwrap().starts_with("error:"));
        assert_eq!(app.game.tuning().get(field), field.attribute().default);
    }
    let reply = read(&mut app, Command::Set { session: std::process::id(), key: "player_speed".into(), value: 4.0 });
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["state"]["error"], false);
    assert_eq!(reply["state"]["fields"][0]["value"], 4.0);
    assert_eq!(reply["state"]["unsaved"], true);
    for value in [7.0, -1.0] {
        let (reply, response) = std::sync::mpsc::channel();
        requests.send(crate::harness::Request {
            command: crate::harness::Command::TuningEdit(Edit::Set(TuningField::PlayerSpeed, value)), reply,
        }).unwrap();
        assert!(!app.drain_harness());
        let response = response.try_recv().unwrap();
        let snapshot = read(&mut app, Command::State {});
        assert_eq!(snapshot["state"]["fields"][0]["value"], 7.0);
        assert_eq!(snapshot["state"]["error"], value < 0.0);
        assert_eq!(snapshot["state"]["message"], response.strip_prefix("error: ").unwrap_or(&response));
    }
}

#[test]
fn expired_companion_edit_cannot_mutate_later() {
    use std::io::{Read, Write};
    let path = std::env::temp_dir().join(format!("arpg-expired-{}.sock", std::process::id()));
    let server = crate::companion::Server::start(path.clone()).unwrap();
    let mut stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
    writeln!(stream, "{}", serde_json::json!({"op":"set","session":std::process::id(),"key":"player_speed","value":4})).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap(); // server's bounded reply wait expires
    assert!(response.contains("did not reply"), "{response}");
    // A stalled game has room for one request, even after its caller times out.
    let mut second = std::os::unix::net::UnixStream::connect(&path).unwrap();
    second.write_all(b"{\"op\":\"state\"}\n").unwrap();
    response.clear();
    second.read_to_string(&mut response).unwrap();
    assert!(response.contains("Game is busy"), "{response}");
    let mut app = App { companion_server: Some(server), ..Default::default() };
    let before = app.game.hash();
    app.drain_companion();
    assert_eq!(app.game.hash(), before);
}

#[test]
fn companion_controls_refuse_stale_sessions_and_performance_trials() {
    use serde_json::json;
    let mut app = App { camera: Some(OrthoCamera::new(800, 600)), ..Default::default() };
    let before = app.game.hash();
    let height = app.camera.as_ref().unwrap().view_height();
    let commands = [
        json!({"op":"restart"}),
        json!({"op":"start_scene", "path":"missing.ron"}),
        json!({"op":"collision", "enabled":true}),
        json!({"op":"vsync", "enabled":false}), json!({"op":"zoom", "direction":"in"}),
        json!({"op":"capture", "path":"unused.png"}),
    ];
    for mut command in commands.clone() {
        command["session"] = json!(0);
        let reply: serde_json::Value = serde_json::from_str(&app.companion_reply(serde_json::from_value(command).unwrap())).unwrap();
        assert_eq!(reply["ok"], false);
        assert!(reply["error"].as_str().unwrap().contains("Game restarted"));
    }
    let spec = crate::trial::Spec::parse("1 60 idle".split_whitespace()).unwrap();
    let (reply, pending) = std::sync::mpsc::channel();
    app.trial = Some(crate::trial::Trial::new(spec, reply, app.game.enemy_count()));
    for command in commands {
        let reply = companion(&mut app, command);
        assert_eq!(reply["ok"], false);
        assert!(reply["error"].as_str().unwrap().contains("performance trial"));
    }
    assert_eq!(app.game.hash(), before);
    assert_eq!(app.run_id, 0);
    assert_eq!(app.camera.as_ref().unwrap().view_height(), height);
    assert!(!app.collision_debug.enabled());
    assert!(app.capture.path().is_none());
    assert!(app.trial.is_some());
    assert!(pending.try_recv().is_err());
}

#[test]
fn keyboard_and_harness_zoom_share_companion_steps_without_gameplay_actions() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App { camera: Some(OrthoCamera::new(800, 600)), harness: Some(rx), ..Default::default() };
    let before = app.game.hash();
    let height = app.camera.as_ref().unwrap().view_height();
    for (closer, farther) in [("+", "-"), ("numpad+", "numpad-")] {
        let key = crate::input::key_named(closer).unwrap();
        assert!(app.on_key(key, true, false));
        let zoomed = app.camera.as_ref().unwrap().view_height();
        assert!((zoomed - height / 1.2).abs() < 0.0001);
        app.on_key(key, true, true);
        app.on_key(key, false, false);
        assert_eq!(app.camera.as_ref().unwrap().view_height(), zoomed);
        let (reply, response) = std::sync::mpsc::channel();
        tx.send(Request { command: Command::Tap(crate::input::key_named(farther).unwrap()), reply }).unwrap();
        assert!(!app.drain_harness());
        app.service_schedule();
        assert_eq!(response.try_recv().unwrap(), "ok");
        assert!((app.camera.as_ref().unwrap().view_height() - height).abs() < 0.0001);
        let actions = app.input.sample();
        for action in arpg_core::Action::ALL {
            assert!(!actions.held(action) && !actions.just_pressed(action) && !actions.just_released(action));
        }
        assert_eq!(app.game.hash(), before);
    }
    for _ in 0..100 { app.on_key(KeyCode::Minus, true, false); }
    assert_eq!(app.camera.as_ref().unwrap().view_height(), 800.0);
    for _ in 0..100 { app.on_key(KeyCode::Equal, true, false); }
    assert_eq!(app.camera.as_ref().unwrap().view_height(), 4.0);
    assert_eq!(app.game.hash(), before);
}

#[test]
fn companion_zoom_uses_the_camera_and_preserves_gameplay() {
    use serde_json::json;
    let mut app = App { camera: Some(OrthoCamera::new(800, 600)), ..Default::default() };
    let before = app.game.hash();
    let height = app.camera.as_ref().unwrap().view_height();
    let closer = companion(&mut app, json!({"op":"zoom", "direction":"in"}));
    assert_eq!(closer["ok"], true);
    let reported = closer["state"]["debug"]["view_height"].as_f64().unwrap() as f32;
    assert!(reported < height);
    assert_eq!(reported, app.camera.as_ref().unwrap().view_height());
    companion(&mut app, json!({"op":"zoom", "direction":"out"}));
    assert!((app.camera.as_ref().unwrap().view_height() - height).abs() < 0.0001);
    assert_eq!(app.game.hash(), before);
    assert_eq!(companion(&mut app, json!({"op":"capture", "path":"unused.png"}))["ok"], false);
    assert!(app.capture.path().is_none());
}

#[test]
fn scene_file_start_is_transactional_and_preserves_tuning() {
    use serde_json::json;
    let directory = std::env::temp_dir().join(format!("arpg-scene-load-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("authored.ron");
    let _ = std::fs::remove_file(&path);
    let mut app = App::default();
    app.start_playtest(pair()).unwrap();
    let tuning = app.game.tuning().with(arpg_game::TuningField::SlamImpulse, 42.0).unwrap();
    app.game.set_tuning(tuning);
    let before = app.game.hash(); let run = app.run_id;
    let refused = companion(&mut app, json!({"op":"start_scene", "path":path}));
    assert_eq!(refused["ok"], false);
    assert_eq!(app.game.hash(), before); assert_eq!(app.run_id, run);
    let document = arpg_content::SceneDocument { scene: pair(), labels: Vec::new() };
    arpg_content::save_scene_document(&path, &document, None).unwrap();
    let started = companion(&mut app, json!({"op":"start_scene", "path":path}));
    assert_eq!(started["ok"], true);
    assert_eq!(app.run_id, run + 1); assert_eq!(app.game.tuning(), tuning);
    std::fs::remove_dir_all(directory).unwrap();
}
