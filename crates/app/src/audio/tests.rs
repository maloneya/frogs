use super::*;
use arpg_core::{AimDir, MoveDir};
use arpg_game::{GameScene};
use arpg_sim::{Accumulator, Dt, Impulse};
use arpg_game::Scene;
use arpg_game::Placed;
use glam::Vec3;

fn intent(movement: Vec3, attack: bool) -> Intent {
    Intent::new(MoveDir::new(movement), AimDir::FORWARD, attack)
}

fn step(game: &mut Game, audio: &mut Audio, input: Intent) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, input);
    let hash = game.hash();
    audio.observe(game, input);
    let cues = audio.cues;
    audio.observe(game, input);
    assert_eq!(audio.cues, cues, "duplicate observation cannot replay a cue");
    assert_eq!(game.hash(), hash, "audio cannot change gameplay");
}

#[test]
fn committed_swings_sound_at_activation_and_misses_have_no_impact() {
    for profile in [AttackProfile::Cleave, AttackProfile::Slam] {
        let mut game = Game::empty();
        let mut audio = Audio::default();
        game.set_attack_profile(profile);
        let attack = profile.resolve(arpg_game::Tuning::default()).unwrap();
        for elapsed in 0..attack.startup() + attack.active() + attack.recovery().get() + 2 {
            step(&mut game, &mut audio, intent(Vec3::ZERO, elapsed == 0));
            if elapsed == 1 { game.set_attack_profile(AttackProfile::Disintegrate); }
            let count = u64::from(elapsed >= attack.startup());
            let cue = if profile == AttackProfile::Cleave { Cue::Cleave } else { Cue::Slam };
            assert_eq!(audio.cues[cue as usize], count, "activation is the first audible tick");
            assert_eq!(audio.cues[Cue::Impact as usize], 0);
        }
    }
}

#[test]
fn a_slam_hitting_multiple_enemies_produces_one_contact_cue() {
    let scene: GameScene = Scene {
        name: "contacts".into(),
        bodies: vec![Placed { pos: (-0.5, 1.2), what: arpg_game::ActorDefinition::passive_hostile() },
            Placed { pos: (0.5, 1.2), what: arpg_game::ActorDefinition::passive_hostile() }],
        ..Default::default()
    }.into();
    let mut game = Game::from_scene(&scene).unwrap();
    game.set_attack_profile(AttackProfile::Slam);
    let mut audio = Audio::default();
    let resolved = AttackProfile::Slam.resolve(arpg_game::Tuning::default()).unwrap();
    for elapsed in 0..resolved.startup() + resolved.active() + 1 {
        step(&mut game, &mut audio, intent(Vec3::ZERO, elapsed == 0));
        assert_eq!(audio.cues[Cue::Impact as usize], u64::from(elapsed >= resolved.startup()));
    }
    assert_eq!(game.struck(), 2);
}

#[test]
fn footsteps_require_walking_progress_and_stop_for_idle_and_channeling() {
    let mut game = Game::empty();
    let mut audio = Audio::default();
    for tick in 1..=STEP_TICKS * 2 {
        step(&mut game, &mut audio, intent(Vec3::X, false));
        assert_eq!(audio.cues[Cue::Footstep as usize], u64::from(tick / STEP_TICKS));
    }
    assert!(game.apply_impulse(game.player_id(), Impulse::try_from((6.0, 0.0)).unwrap()));
    for _ in 0..STEP_TICKS { step(&mut game, &mut audio, intent(Vec3::ZERO, false)); }
    assert_eq!(audio.cues[Cue::Footstep as usize], 2, "carried motion is not walking");
    game.set_attack_profile(AttackProfile::Disintegrate);
    for _ in 0..STEP_TICKS {
        step(&mut game, &mut audio, intent(Vec3::X, false).with_attack_held(true));
    }
    assert!(game.attack_status().channeling);
    assert_eq!(audio.cues[Cue::Footstep as usize], 2);
    for _ in 0..STEP_TICKS { step(&mut game, &mut audio, intent(Vec3::X, false)); }
    assert_eq!(audio.cues[Cue::Footstep as usize], 3);
}

#[test]
fn holding_movement_against_the_arena_wall_is_silent() {
    let mut game = Game::empty();
    let mut audio = Audio::default();
    // Reach the wall first, then establish a fresh presentation observation.
    for _ in 0..1000 { step(&mut game, &mut audio, intent(Vec3::X, false)); }
    audio.reset(&game);
    for _ in 0..STEP_TICKS * 2 { step(&mut game, &mut audio, intent(Vec3::X, false)); }
    assert_eq!(audio.cues[Cue::Footstep as usize], 0);
}

#[test]
fn health_loss_sounds_once_and_restart_resets_observations() {
    let scene = GameScene {
        engine: Scene { name: "attacker".into(),
            bodies: vec![Placed { pos: (0.0, 0.55), what: arpg_game::ActorDefinition::attacker() }],
            ..Default::default() },
        ..Default::default()
    };
    let mut game = Game::from_scene(&scene).unwrap();
    let mut audio = Audio::default();
    for _ in 0..37 { step(&mut game, &mut audio, intent(Vec3::ZERO, false)); }
    assert_eq!(audio.cues[Cue::Hurt as usize], 1);
    for _ in 0..5 { step(&mut game, &mut audio, intent(Vec3::ZERO, false)); }
    assert_eq!(audio.cues[Cue::Hurt as usize], 1);
    game.restart().unwrap();
    audio.reset(&game);
    assert_eq!(audio.cues, [0; Cue::ALL.len()]);
    for _ in 0..37 { step(&mut game, &mut audio, intent(Vec3::ZERO, false)); }
    assert_eq!(audio.cues[Cue::Hurt as usize], 1, "a reused tick is fresh after restart");
}

#[test]
fn beam_pulses_sound_once_for_many_targets_including_defeat_and_misses_are_silent() {
    let mut game = Game::empty();
    let mut audio = Audio::default();
    for z in [3.0, 6.0] { game.place(glam::Vec2::new(0.0, z), arpg_game::ActorDefinition::passive_hostile()).unwrap(); }
    game.set_attack_profile(AttackProfile::Disintegrate);
    for tick in 0..30 {
        step(&mut game, &mut audio, intent(Vec3::ZERO, false).with_attack_held(true));
        assert_eq!(audio.cues[Cue::Impact as usize], (tick / arpg_game::Disintegrate::HIT_EVERY + 1).min(3) as u64);
        assert_eq!(audio.cues[Cue::Cleave as usize], 0);
    }
    assert_eq!(game.enemy_count(), 0);
    step(&mut game, &mut audio, intent(Vec3::ZERO, false));
    assert_eq!(audio.cues[Cue::Impact as usize], 3);
}
