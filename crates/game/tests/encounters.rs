//! Encounter composition through Game, with real combat and full lifetime checks.
use arpg_game::{Placed, SourceSpec, ActorDefinition};
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{
    DefeatObjective, DoorSpec, DoorState, EncounterAction as Action, EncounterObjective,
    EncounterPhase as Phase, EncounterSpec, EncounterStart, Game, GameScene,
};
use arpg_sim::{Accumulator, Condition, Dt};
use arpg_game::{AttackProfile};
use glam::{Vec2, Vec3};

fn source(pos: (f32, f32)) -> SourceSpec {
    SourceSpec {
        pos,
        radius: 0.0,
        every: 10_000,
        when: Condition::Always,
        what: ActorDefinition::passive_hostile(),
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
        on_start: vec![],
        on_success: vec![Action::UnlockDoor(door)],
        on_failure: vec![],
    }
}
fn scene() -> GameScene {
    let mut scene = GameScene::default();
    scene.engine.bodies = vec![
        Placed {
            pos: (1.0, 0.0),
            what: ActorDefinition::prop().interactive(),
        },
        Placed {
            pos: (-1.0, 0.0),
            what: ActorDefinition::prop().interactive(),
        },
        Placed {
            pos: (20.0, 10.0),
            what: ActorDefinition::prop(),
        },
        Placed {
            pos: (-20.0, 10.0),
            what: ActorDefinition::prop(),
        },
    ];
    scene.engine.sources = vec![source((0.0, 4.0)), source((-50.0, 0.0))];
    scene.doors = vec![DoorSpec { body: 2, panel: arpg_game::DoorPanel::default() }, DoorSpec { body: 3, panel: arpg_game::DoorPanel::default() }];
    for door in &scene.doors { scene.engine.bodies[door.body].what.physical = arpg_sim::Template::BODY; }
    scene.encounters = vec![trial("East", 0, 0, 0, 699), trial("West", 1, 1, 1, 697)];
    scene
}
fn step(game: &mut Game, tick: u64, interact: bool) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)
        .with_attack_held(tick >= 675).with_interact(interact));
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
    game.set_attack_profile(AttackProfile::Disintegrate);
    for tick in 0..699 {
        step(&mut game, tick, [0, 2].contains(&tick));
    }
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Running);
    assert_eq!(game.encounter_state(id, 0).unwrap().remaining_ticks, 1);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Latched));
    step(&mut game, 699, true); // Beam pulse lands on the final objective tick.
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Succeeded);
    assert_eq!(game.encounter_state(id, 1).unwrap().phase, Phase::Failed);
    assert_eq!(game.encounter_state(id, 1).unwrap().defeated, 0);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Released));
    assert_eq!(game.door_state(id, 1), Some(DoorState::Latched));
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
        "releasing a latch preserves its physical panel"
    );
    step(&mut game, 701, true);
    step(&mut game, 702, false);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Released));
    assert!(game.is_alive(door));
    assert!(game.despawn_body(door));
    let replacement = game.place(Vec2::new(20.0, 10.0), ActorDefinition::prop()).unwrap();
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
    game.set_attack_profile(AttackProfile::Disintegrate);
    for tick in 0..701 {
        step(&mut game, tick, tick == 0);
    }
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Failed);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Latched));
}

#[test]
fn despawn_and_recycled_identity_do_not_award_progress_and_endpoint_loss_stops_sources() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    step(&mut game, 0, true);
    step(&mut game, 1, false);
    let spawned = *game.scene_bodies(id).unwrap().last().unwrap();
    assert!(game.despawn_body(spawned));
    let replacement = game.place(Vec2::new(0.0, 4.0), ActorDefinition::passive_hostile()).unwrap();
    assert_ne!(spawned, replacement);
    step(&mut game, 2, false);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    let door = game.scene_bodies(id).unwrap()[2];
    assert!(game.despawn_body(door));
    let other = game.place(Vec2::new(20.0, 10.0), ActorDefinition::prop()).unwrap();
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
    assert!(game.request_spawn(Vec2::new(20.0, 20.0), ActorDefinition::passive_hostile()).is_some());
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
    s.encounters[0].on_start = vec![Action::UnlockDoor(0), Action::UnlockDoor(0)];
    invalid.push(s);
    let mut s = scene();
    let EncounterObjective::Defeat(o) = &mut s.encounters[0].objective;
    o.sources.push(0);
    invalid.push(s);
    let mut s = scene();
    let EncounterObjective::Defeat(o) = &mut s.encounters[0].objective;
    o.sources = vec![100];
    invalid.push(s);
    let mut s = scene();
    s.engine.sources[0].enabled = true;
    invalid.push(s);
    let mut s = scene();
    s.engine.sources[0].what = ActorDefinition::prop();
    invalid.push(s);
    let mut s = scene();
    s.engine.bodies[0].what = ActorDefinition::prop();
    invalid.push(s);
    let mut s = scene();
    let EncounterObjective::Defeat(o) = &mut s.encounters[1].objective;
    o.sources = vec![0];
    invalid.push(s);
    let mut s = scene();
    s.engine.sources[0].pos.0 = f32::NAN;
    invalid.push(s);
    let mut s = scene();
    s.engine.bodies[2].what.physical = arpg_sim::Template::BLOCK.character();
    invalid.push(s);
    for invalid in invalid {
        assert!(game.load_scene(&invalid).is_err());
        assert_eq!(observed(&game), before);
        assert!(game.start_scene(&invalid).is_err());
        assert_eq!(observed(&game), before);
    }
    step(&mut game, 1, false);
    assert_eq!(game.enemy_count(), 2);
    game.restart().unwrap();
    assert_eq!(observed(&game), observed(&Game::from_scene(&scene()).unwrap()));
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
                .place(Vec2::new(1.0, 0.0), ActorDefinition::prop().interactive())
                .unwrap();
        } else {
            let removed_source = game.scene_sources(id).unwrap()[0];
            assert!(game.remove_source(removed_source));
            let _ = game.add_source(source((0.0, 4.0)).try_into().unwrap());
        }
        step(&mut game, 1, true);
        assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Orphaned);
        assert_eq!(game.door_state(id, 0), Some(DoorState::Latched));
    }
}

#[test]
fn defeating_an_unrelated_replacement_never_counts_as_a_source_defeat() {
    let mut authored = scene();
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.within_ticks = 800.try_into().unwrap();
    let mut game = Game::from_scene(&authored).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    game.set_attack_profile(AttackProfile::Disintegrate);
    step(&mut game, 0, true);
    step(&mut game, 1, false);
    let spawned = *game.scene_bodies(id).unwrap().last().unwrap();
    assert!(game.despawn_body(spawned));
    let replacement = game.place(Vec2::new(0.0, 4.0), ActorDefinition::passive_hostile()).unwrap();
    for tick in 2..700 {
        step(&mut game, tick, false);
    }
    assert_eq!(game.health(replacement), Some(0));
    assert_eq!(game.defeats().len(), 1);
    assert_eq!(game.defeats()[0].source, None);
    assert_eq!(game.encounter_state(id, 0).unwrap().defeated, 0);
    assert_eq!(game.encounter_state(id, 0).unwrap().phase, Phase::Running);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Latched));
    step(&mut game, 700, false);
    assert_eq!(game.health(replacement), Some(0));
    assert!(game.defeats().is_empty(), "retirement cannot count the death again");
}

#[test]
fn a_source_group_accepts_either_producer_and_stops_both_on_success() {
    let mut authored = scene();
    authored.encounters.truncate(1);
    authored.engine.sources.swap(0, 1);
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.sources.push(1);
    let mut game = Game::from_scene(&authored).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    let sources = game.scene_sources(id).unwrap().to_vec();
    game.set_attack_profile(AttackProfile::Disintegrate);
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
fn unlocking_releases_the_latch_and_movement_opens_the_surviving_panel() {
    let mut authored = scene();
    authored.engine.bodies[2].pos = (0.0, 2.0);
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.within_ticks = 1.try_into().unwrap();
    authored.encounters[0].on_failure = vec![Action::UnlockDoor(0)];
    let mut game = Game::from_scene(&authored).unwrap();
    let id = game.scene_instances().next().unwrap().0;
    let body = game.scene_bodies(id).unwrap()[2];
    step(&mut game, 0, true);
    step(&mut game, 1, false);
    assert_eq!(game.door_state(id, 0), Some(DoorState::Released));
    assert_eq!(game.joint(body).unwrap().angle(), 0.0, "release itself adds no motion");
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    for _ in 0..120 {
        game.step(dt, Intent::new(MoveDir::new(Vec3::Z), AimDir::FORWARD, false));
    }
    assert!(game.is_alive(body));
    assert!(game.joint(body).unwrap().angle().abs() > 0.5);
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

#[test]
fn a_hit_admitted_on_the_unlocking_tick_swings_the_released_panel() {
    // Prediction: activation on tick 0 times the trial out on tick 10. A slam
    // pressed on tick 1 lands on tick 10 too, after that tick's contact solve,
    // so the held latch has not absorbed it when the timeout releases it.
    // The released panel takes the admitted impulse as spin.
    let mut authored = scene();
    authored.engine.bodies[2].pos = (0.0, 1.2);
    authored.doors[0].panel = arpg_game::DoorPanel::new(2.0, 0.2, 0.0, -1.55, 1.55, 100.0).unwrap();
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.within_ticks = 10.try_into().unwrap();
    authored.encounters[0].on_failure = vec![Action::UnlockDoor(0)];
    let mut game = Game::from_scene(&authored).unwrap();
    game.set_attack_profile(AttackProfile::Slam);
    let id = game.scene_instances().next().unwrap().0;
    let door = game.scene_bodies(id).unwrap()[2];
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    for tick in 0..10 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, tick == 1).with_interact(tick == 0));
        assert_eq!(game.door_state(id, 0), Some(DoorState::Latched), "tick {tick}");
    }
    game.clear_trace();
    step(&mut game, 10, false);
    assert!(game.trace().iter().any(|(tick, event)| tick == 10
        && event == arpg_sim::Event::Impulsed { id: door, source: Some(game.player_id()), impulse: arpg_sim::Impulse::try_from((0.0, 72.0)).unwrap() }));
    assert_eq!(game.door_state(id, 0), Some(DoorState::Released));
    let panel = game.joint(door).unwrap();
    assert_eq!(panel.angle(), 0.0, "release and admission add no displacement");
    // Positive angles turn the free edge toward -Z; the slam pushes it toward +Z.
    assert!(panel.rate() < 0.0, "the admitted slam survives release as spin");
    step(&mut game, 11, false);
    assert!(game.joint(door).unwrap().angle() < 0.0);
}

#[test]
fn source_group_starts_once_and_pending_activation_is_discarded_by_restart() {
    // Prediction: activation completes on tick 0; both sources start on tick 1
    // in objective order. Disabling one externally is not undone by the pass.
    let mut authored = scene();
    authored.encounters.truncate(1);
    authored.encounters[0].on_start = vec![Action::UnlockDoor(0)];
    let EncounterObjective::Defeat(objective) = &mut authored.encounters[0].objective;
    objective.sources = vec![1, 0];
    let mut game = Game::from_scene(&authored).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let sources = game.scene_sources(owner).unwrap().to_vec();
    step(&mut game, 0, true);
    assert_eq!(game.enemy_count(), 0);
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, Phase::Ready);
    assert_eq!(game.door_state(owner, 0), Some(DoorState::Latched));
    game.restart().unwrap();
    step(&mut game, 0, false);
    step(&mut game, 1, false);
    assert_eq!(game.enemy_count(), 0, "restart discards pending activation");
    step(&mut game, 2, true);
    game.clear_trace();
    step(&mut game, 3, false);
    assert_eq!(game.enemy_count(), 2);
    assert_eq!(game.door_state(owner, 0), Some(DoorState::Released));
    let enabled: Vec<_> = game.trace().iter().filter_map(|(_, event)| match event {
        arpg_sim::Event::SourceEnablementChanged { id, enabled: true } => Some(id),
        _ => None,
    }).collect();
    assert_eq!(enabled, vec![sources[1], sources[0]]);
    assert!(game.set_source_enabled(sources[0], false));
    step(&mut game, 4, true);
    step(&mut game, 5, false);
    assert!(!game.source_state(sources[0]).unwrap().enabled);
    assert!(game.source_state(sources[1]).unwrap().enabled);
    game.restart().unwrap();
    assert_eq!(observed(&game), observed(&Game::from_scene(&authored).unwrap()));
}

#[test]
fn encounter_endpoints_and_cached_relationships_affect_hash_without_changing_physics() {
    let mut authored = scene();
    authored.encounters.truncate(1);
    let mut other = authored.clone();
    other.encounters[0].start = EncounterStart::Activated(1);
    let mut a = Game::from_scene(&authored).unwrap();
    let mut b = Game::from_scene(&other).unwrap();
    assert_eq!(a.engine_hash(), b.engine_hash());
    assert_ne!(a.hash(), b.hash());
    for game in [&mut a, &mut b] {
        let owner = game.scene_instances().next().unwrap().0;
        assert!(game.evict_scene(owner));
    }
    assert_eq!(a.engine_hash(), b.engine_hash());
    assert_ne!(a.hash(), b.hash(), "cached encounter endpoint changes restart behavior");
}
