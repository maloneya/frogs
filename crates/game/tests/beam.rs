//! Channel lifecycle and physical composition through the public Game boundary.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{Game, GameScene};
use arpg_sim::{
    Accumulator, AttackPhase, AttackProfile, Beam, Dt, Impulse, Placed, Scene, Template,
};
use glam::{Vec2, Vec3};

fn step(game: &mut Game, held: bool) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(
        dt,
        Intent::new(MoveDir::new(Vec3::X), AimDir::FORWARD, false).with_attack_held(held),
    );
}

#[test]
fn channel_allows_physics_and_restart_clears_it_while_refusal_preserves_it() {
    let scene: GameScene = Scene::default().into();
    let mut game = Game::from_scene(&scene).unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    assert!(game.apply_impulse(game.player_id(), Impulse::try_from((6.0, 0.0)).unwrap()));
    let expected = 6.0 * game.motion(game.player_id()).unwrap().inverse_mass() * Dt::SECS;
    step(&mut game, true);
    assert!(
        (game.player_pos().x - expected).abs() < 1e-6,
        "carried motion survives channel, powered walking does not"
    );
    assert!(game.beam().is_some());
    let mut out = Report::default();
    game.report(&mut out);
    let out = out.finish();
    assert!(out.contains("\"channeling\":true"));
    assert!(out.contains("\"hit_every_ticks\":12"));
    let before = game.hash();
    let bad: GameScene = Scene {
        bodies: vec![Placed {
            pos: (f32::NAN, 0.0),
            what: Template::BODY,
        }],
        ..Scene::default()
    }
    .into();
    assert!(game.start_scene(&bad).is_err());
    assert!(game.load_scene(&bad).is_err());
    assert_eq!(game.hash(), before);
    assert!(game.beam().is_some());
    game.restart().unwrap();
    assert!(game.beam().is_none());
    assert!(!game.attack_status().channeling);
    assert_eq!(game.hash(), Game::from_scene(&scene).unwrap().hash());
}

#[test]
fn scene_eviction_and_slot_reuse_do_not_leave_beam_target_memory() {
    let mut game = Game::empty();
    let scene: GameScene = Scene {
        bodies: vec![Placed {
            pos: (0.0, 4.0),
            what: Template::BODY,
        }],
        ..Scene::default()
    }
    .into();
    let id = game.load_scene(&scene).unwrap();
    let old = game.scene_bodies(id).unwrap()[0];
    game.set_attack_profile(AttackProfile::Disintegrate);
    step(&mut game, true);
    assert_eq!(game.health(old), Some(2));
    assert_eq!(game.beam_hits(), &[Vec2::new(0.0, 4.0)]);
    assert!(game.evict_scene(id));
    assert_eq!(game.beam_hits(), &[Vec2::new(0.0, 4.0)], "historical outcome survives removal");
    let new = game.place(Vec2::new(0.0, 4.0), Template::BODY).unwrap();
    assert_ne!(old, new);
    for _ in 1..Beam::HIT_EVERY {
        step(&mut game, true);
    }
    assert_eq!(game.health(new), Some(3));
    assert!(game.beam_hits().is_empty(), "non-pulse ticks clear prior hit points");
    step(&mut game, true);
    assert_eq!(game.health(old), None);
    assert_eq!(game.health(new), Some(2));
    game.set_attack_profile(AttackProfile::Cleave);
    step(&mut game, true);
    assert!(game.beam().is_none());
    assert_eq!(game.attack_status().phase, AttackPhase::Idle);
}

#[test]
fn beam_waits_for_the_entire_committed_swing_and_release_resets_cadence() {
    let mut game = Game::empty();
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, true));
    let swing = AttackProfile::Cleave.resolve(arpg_sim::Tuning::default()).unwrap();
    let duration = swing.startup() + swing.active() + swing.recovery().get();
    game.set_attack_profile(AttackProfile::Disintegrate);
    for _ in 1..duration {
        step(&mut game, true);
        assert!(game.beam().is_none());
    }
    step(&mut game, true);
    assert!(game.beam().is_some());
    step(&mut game, false);
    assert!(game.beam().is_none());
    let target = game
        .place(Vec2::new(game.player_pos().x, 4.0), Template::BODY)
        .unwrap();
    step(&mut game, true);
    assert_eq!(
        game.health(target),
        Some(2),
        "a fresh hold pulses immediately"
    );
}

#[test]
fn beam_hit_outcomes_are_hashed_reported_and_restart_clears_them() {
    let scene: GameScene = Scene {
        bodies: vec![Placed { pos: (0.0, 3.0), what: Template::BODY }],
        ..Default::default()
    }.into();
    let mut game = Game::from_scene(&scene).unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    step(&mut game, true);
    assert_eq!(game.beam_hits(), &[Vec2::new(0.0, 3.0)]);
    let mut report = Report::default();
    game.report(&mut report);

    let report = report.finish();
    assert!(report.contains("\"hits\":1"));
    assert!(report.contains("\"hit_positions\":{\"0\":[0.0000,0.0000,3.0000]}"));
    let hash = game.hash();
    let invalid: GameScene = Scene {
        bodies: vec![Placed { pos: (f32::NAN, 0.0), what: Template::BODY }],
        ..Default::default()
    }.into();
    assert!(game.start_scene(&invalid).is_err());
    assert_eq!(game.hash(), hash);
    assert_eq!(game.beam_hits(), &[Vec2::new(0.0, 3.0)]);
    game.restart().unwrap();
    assert!(game.beam_hits().is_empty());
}
