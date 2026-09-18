//! Encounter composition through Game, with real combat and full lifetime checks.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{
    DefeatObjective, DoorSpec, DoorState, EncounterAction as Action, EncounterObjective,
    EncounterPhase as Phase, EncounterSpec, EncounterStart, Game, GameScene, SourceControlSpec,
};
use arpg_sim::{Accumulator, AttackProfile, Condition, Dt, Placed, SourceSpec, Template};
use glam::{Vec2, Vec3};

fn source(pos: (f32, f32)) -> SourceSpec {
    SourceSpec {
        pos,
        radius: 0.0,
        every: 10_000,
        when: Condition::Always,
        what: Template::BODY,
        enabled: false,
    }
}
fn trial(name: &str, body: usize, source: usize, door: usize, ticks: u32) -> EncounterSpec {
    EncounterSpec {
        name: name.into(),
        start: EncounterStart::Activated(body),
        objective: EncounterObjective::Defeat(DefeatObjective {
            sources: vec![source],
            count: 1.try_into().unwrap(),
            within_ticks: ticks.try_into().unwrap(),
        }),
        on_start: vec![Action::EnableSource(source)],
        on_success: vec![Action::UnlockDoor(door)],
        on_failure: vec![],
    }
}
fn scene() -> GameScene {
    let mut scene = GameScene::default();
    scene.engine.bodies = vec![
        Placed {
            pos: (1.0, 0.0),
            what: Template::BLOCK.interactive(),
        },
        Placed {
            pos: (-1.0, 0.0),
            what: Template::BLOCK.interactive(),
        },
        Placed {
            pos: (1.4, 94.5),
            what: Template::BLOCK,
        },
        Placed {
            pos: (-20.0, 0.0),
            what: Template::BLOCK,
        },
    ];
    scene.engine.sources = vec![source((0.0, 95.7)), source((-50.0, 0.0))];
    scene.doors = vec![DoorSpec { body: 2 }, DoorSpec { body: 3 }];
    scene.encounters = vec![trial("East", 0, 0, 0, 699), trial("West", 1, 1, 1, 697)];
    scene
}
fn step(game: &mut Game, tick: u64, interact: bool) {
    let movement = if tick < 630 {
        MoveDir::new(Vec3::Z)
    } else {
        MoveDir::NONE
    };
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(
        dt,
        Intent::new(movement, AimDir::FORWARD, [630, 660, 690].contains(&tick))
            .with_interact(interact),
    );
}
fn observed(game: &Game) -> (u64, String, String) {
    let mut out = Report::default();
    game.report(&mut out);
    (game.hash(), out.finish(), game.render_trace_since(0))
}

#[test]
fn two_trials_count_their_own_defeats_and_final_tick_success_unlocks_only_its_door() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    let door = game.scene_bodies(id).unwrap()[2];
    game.set_attack_profile(AttackProfile::Slam);
    for tick in 0..699 {
        step(&mut game, tick, [0, 2].contains(&tick));
    }
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Running);
    assert_eq!(game.encounter_state(id, 0).unwrap().remaining_ticks, 1);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Locked));
    step(&mut game, 699, true); // Locked door cannot consume this press.
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Succeeded);
    assert_eq!(game.encounter_state(id, 1).unwrap().phase, Phase::Failed);
    assert_eq!(game.encounter_state(id, 1).unwrap().defeated, 0);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Unlocked));
    assert_eq!(game.door_state(id, 1), Some(DoorState::Locked));
    for source in game.scene_sources(id).unwrap() {
        assert!(!game.source_state(*source).unwrap().enabled);
    }
    let outcomes = game.defeats().to_vec();
    let hash = game.hash();
    game.clear_trace();
    assert_eq!(game.hash(), hash);
    assert_eq!(game.defeats(), outcomes);
    step(&mut game, 700, false);
    assert!(
        game.is_alive(door),
        "press while locked must not open later"
    );
    step(&mut game, 701, true);
    step(&mut game, 702, false);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Open));
    assert!(!game.is_alive(door));
    let replacement = game.place(Vec2::new(1.4, 94.5), Template::BLOCK).unwrap();
    step(&mut game, 703, true);
    assert!(game.is_alive(replacement));
}

#[test]
fn timeout_one_tick_before_defeat_never_rewards_a_late_kill() {
    let mut authored = scene();
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.within_ticks = 698.try_into().unwrap();
    let mut game = Game::from_scene(&authored).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    game.set_attack_profile(AttackProfile::Slam);
    for tick in 0..701 {
        step(&mut game, tick, tick == 0);
    }
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Failed);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Locked));
}

#[test]
fn despawn_and_recycled_identity_do_not_award_progress_and_endpoint_loss_stops_sources() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    step(&mut game, 0, true);
    step(&mut game, 1, false);
    let spawned = *game.scene_bodies(id).unwrap().last().unwrap();
    assert!(game.despawn_body(spawned));
    let replacement = game.place(Vec2::new(0.0, 95.7), Template::BODY).unwrap();
    assert_ne!(spawned, replacement);
    step(&mut game, 2, false);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    let door = game.scene_bodies(id).unwrap()[2];
    assert!(game.despawn_body(door));
    let other = game.place(Vec2::new(1.4, 94.5), Template::BLOCK).unwrap();
    step(&mut game, 3, false);
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Orphaned);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Missing));
    assert!(game.is_alive(other));
    let source = game.scene_sources(id).unwrap()[0];
    assert!(!game.source_state(source).unwrap().enabled);
}

#[test]
fn load_rejection_is_atomic_even_with_pending_spawns_and_activation() {
    let mut game = Game::from_scene(&scene()).unwrap();
    step(&mut game, 0, true);
    assert!(game.request_spawn(Vec2::new(20.0, 20.0), Template::BODY));
    let before = observed(&game);
    let mut invalid = Vec::new();
    let mut s = scene();
    s.encounters[0].start = EncounterStart::Activated(100);
    invalid.push(s);
    let mut s = scene();
    s.encounters[1].name = "East".into();
    invalid.push(s);
    let mut s = scene();
    s.encounters[0].name = "\n".into();
    invalid.push(s);
    let mut s = scene();
    s.doors[0].body = 0;
    invalid.push(s);
    let mut s = scene();
    s.doors.push(s.doors[0]);
    invalid.push(s);
    let mut s = scene();
    s.encounters[1].on_success = vec![Action::UnlockDoor(0)];
    invalid.push(s);
    let mut s = scene();
    s.encounters[0].on_success = vec![Action::UnlockDoor(100)];
    invalid.push(s);
    let mut s = scene();
    s.encounters[0].on_start.push(Action::EnableSource(0));
    invalid.push(s);
    let mut s = scene();
    s.encounters[0].on_start.clear();
    invalid.push(s);
    let mut s = scene();
    s.encounters[0].on_failure.push(Action::EnableSource(0));
    invalid.push(s);
    let mut s = scene();
    s.engine.sources[0].enabled = true;
    invalid.push(s);
    let mut s = scene();
    s.engine.sources[0].what = Template::BLOCK;
    invalid.push(s);
    let mut s = scene();
    s.source_controls
        .push(SourceControlSpec { body: 0, source: 0 });
    invalid.push(s);
    let mut s = scene();
    let EncounterObjective::Defeat(o) = &mut s.encounters[1].objective;
    o.sources = vec![0];
    invalid.push(s);
    let mut s = scene();
    s.engine.sources[0].pos.0 = f32::NAN;
    invalid.push(s);
    for invalid in invalid {
        assert!(game.load_scene(&invalid).is_err());
        assert_eq!(observed(&game), before);
        assert!(game.start_scene(&invalid).is_err());
        assert_eq!(observed(&game), before);
    }
    step(&mut game, 1, false);
    assert_eq!(game.enemy_count(), 2);
}

#[test]
fn independent_scene_copies_eviction_and_restart_close_the_lifecycle() {
    let authored = scene();
    let mut game = Game::from_scene(&authored).unwrap();
    let a = game.scene_instances().next().unwrap().0;
    let b = game.load_scene(&authored).unwrap();
    step(&mut game, 0, true);
    step(&mut game, 1, false);
    assert_eq!(game.encounter_state(a, 0).unwrap().phase, Phase::Running);
    assert_eq!(game.encounter_state(b, 0).unwrap().phase, Phase::Ready);
    assert!(game.evict_scene(a));
    assert_eq!(game.encounter_state(a, 0), None);
    assert_eq!(game.door_state(a, 0), None);
    assert_eq!(game.encounter_state(b, 0).unwrap().phase, Phase::Ready);
    game.restart().unwrap();
    assert_eq!(
        observed(&game),
        observed(&Game::from_scene(&authored).unwrap())
    );
}

#[test]
fn removing_a_pending_start_or_source_cannot_retarget_a_replacement() {
    for remove_start in [true, false] {
        let mut game = Game::from_scene(&scene()).unwrap();
        let id = game.scene_instances().next().unwrap().0;
        step(&mut game, 0, true);
        if remove_start {
            let start = game.scene_bodies(id).unwrap()[0];
            assert!(game.despawn_body(start));
            let _ = game
                .place(Vec2::new(1.0, 0.0), Template::BLOCK.interactive())
                .unwrap();
        } else {
            let removed_source = game.scene_sources(id).unwrap()[0];
            assert!(game.remove_source(removed_source));
            let _ = game.add_source(source((0.0, 95.7)).into());
        }
        step(&mut game, 1, true);
        assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Orphaned);
        assert_eq!(game.door_state(id, 0), Some(DoorState::Locked));
    }
}

#[test]
fn defeating_an_unrelated_replacement_never_counts_as_a_source_defeat() {
    let mut authored = scene();
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.within_ticks = 800.try_into().unwrap();
    let mut game = Game::from_scene(&authored).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    game.set_attack_profile(AttackProfile::Slam);
    step(&mut game, 0, true);
    step(&mut game, 1, false);
    let spawned = *game.scene_bodies(id).unwrap().last().unwrap();
    assert!(game.despawn_body(spawned));
    let replacement = game.place(Vec2::new(0.0, 95.7), Template::BODY).unwrap();
    for tick in 2..700 {
        step(&mut game, tick, false);
    }
    assert!(!game.is_alive(replacement));
    assert_eq!(game.defeats().len(), 1);
    assert_eq!(game.defeats()[0].source, None);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Running);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Locked));
}

#[test]
fn a_source_group_accepts_either_producer_and_stops_both_on_success() {
    let mut authored = scene();
    authored.encounters.truncate(1);
    authored.engine.sources.swap(0, 1);
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.sources.push(1);
    authored.encounters[0]
        .on_start
        .push(Action::EnableSource(1));
    let mut game = Game::from_scene(&authored).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    let sources = game.scene_sources(id).unwrap().to_vec();
    game.set_attack_profile(AttackProfile::Slam);
    for tick in 0..700 {
        step(&mut game, tick, tick == 0);
    }
    assert_eq!(game.defeats()[0].source, Some(sources[1]));
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Succeeded);
    for source in sources {
        assert!(!game.source_state(source).unwrap().enabled);
    }
    game.restart().unwrap();
    assert_eq!(
        observed(&game),
        observed(&Game::from_scene(&authored).unwrap())
    );
}

#[test]
fn opening_is_the_physical_change_unlocking_alone_still_blocks_the_player() {
    let mut authored = scene();
    authored.engine.bodies[2].pos = (0.0, 2.0);
    // Short failure unlocks the door through the same typed effect as success.
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.within_ticks = 1.try_into().unwrap();
    authored.encounters[0].on_failure = vec![Action::UnlockDoor(0)];
    let mut game = Game::from_scene(&authored).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    for tick in 0..20 {
        step(&mut game, tick, tick == 0);
    }
    assert_eq!(game.door_state(id, 0), Some(DoorState::Unlocked));
    assert!(game.player_pos().z < 2.0);
    step(&mut game, 20, true);
    step(&mut game, 21, false);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Open));
    for tick in 22..42 {
        step(&mut game, tick, false);
    }
    assert!(game.player_pos().z > 3.0);
}

#[test]
fn live_capacity_rejection_preserves_instances_and_is_released_on_eviction() {
    let authored = scene();
    let mut game = Game::empty();
    let mut ids = Vec::new();
    for _ in 0..arpg_game::MAX_ENCOUNTERS / 2 {
        ids.push(game.load_scene(&authored).unwrap());
    }
    let before = observed(&game);
    assert!(game.load_scene(&authored).is_err());
    assert_eq!(observed(&game), before);
    assert!(game.evict_scene(ids[0]));
    assert!(game.load_scene(&authored).is_ok());
}
