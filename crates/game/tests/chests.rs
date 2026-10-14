//! Chests retain their bodies and own their complete scene lifetime.
use arpg_game::{Placed, SourceSpec, ActorDefinition};
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{ChestSpec, ChestState, DefeatObjective, DoorSpec, EncounterAction as Action,
    EncounterObjective, EncounterPhase, EncounterSpec, EncounterStart, Game, GameScene};
use arpg_sim::{Accumulator, AttackProfile, Condition, Dt, };
use glam::Vec2;

fn scene() -> GameScene {
    GameScene {
        engine: arpg_game::Scene {
            bodies: vec![
                Placed { pos: (1.2, 0.0), what: ActorDefinition::prop().interactive() },
                Placed { pos: (-1.2, 0.0), what: ActorDefinition::prop() },
            ],
            sources: vec![SourceSpec { pos: (0.0, 1.0), radius: 0.0, every: 10_000,
                when: Condition::Always, what: ActorDefinition::hostile(), enabled: false }],
            ..Default::default()
        },
        chests: vec![ChestSpec { body: 1 }],
        encounters: vec![EncounterSpec {
            name: "Chest trial".into(), start: EncounterStart::Activated(0),
            objective: EncounterObjective::Defeat(DefeatObjective { sources: vec![0],
                count: 1.try_into().unwrap(), within_ticks: 300.try_into().unwrap() }),
            on_start: vec![], on_success: vec![Action::UnlockChest(0)],
            on_failure: vec![],
        }],
        ..Default::default()
    }
}
fn step(game: &mut Game, attack: bool, interact: bool) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, attack).with_interact(interact));
}
fn observed(game: &Game) -> (u64, String, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish(), game.render_trace_since(0))
}

#[test]
fn success_requires_a_fresh_interaction_and_opening_keeps_the_body() {
    // Prediction: repeated Slam attacks defeat the returning seeker within 300
    // ticks. Pressing E on every combat tick cannot queue opening while locked.
    let mut game = Game::from_scene(&scene()).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let chest = game.scene_bodies(owner).unwrap()[1];
    game.set_attack_profile(AttackProfile::Slam);
    for tick in 0..300 {
        assert_eq!(game.chest_state(owner, 0), Some(ChestState::Locked));
        step(&mut game, tick % 30 == 10, true);
        if game.encounter_state(owner, 0).unwrap().phase == EncounterPhase::Succeeded { break; }
    }
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, EncounterPhase::Succeeded);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Unlocked));
    step(&mut game, false, false);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Unlocked));
    step(&mut game, false, true);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Unlocked));
    step(&mut game, false, false);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Open));
    assert_eq!(game.chest_state_for_body(chest), Some(ChestState::Open));
    assert!(game.is_alive(chest));
    let hash = game.hash();
    game.clear_trace();
    assert_eq!(game.hash(), hash);
    assert!(game.despawn_body(chest));
    let replacement = game.place(Vec2::new(-1.2, 0.0), ActorDefinition::prop().interactive()).unwrap();
    step(&mut game, false, true);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Missing));
    assert_eq!(game.chest_state_for_body(replacement), None);
    assert!(game.is_alive(replacement));
    game.restart().unwrap();
    assert_eq!(observed(&game), observed(&Game::from_scene(&scene()).unwrap()));
}

#[test]
fn missing_reward_orphans_encounter_and_eviction_releases_only_its_chests() {
    let mut game = Game::from_scene(&scene()).unwrap();
    let a = game.scene_instances().next().unwrap().0;
    let b = game.load_scene(&scene()).unwrap();
    step(&mut game, false, true);
    let chest = game.scene_bodies(a).unwrap()[1];
    assert!(game.despawn_body(chest));
    let replacement = game.place(Vec2::new(-1.2, 0.0), ActorDefinition::prop()).unwrap();
    step(&mut game, false, false);
    assert_eq!(game.encounter_state(a, 0).unwrap().phase, EncounterPhase::Orphaned);
    assert_eq!(game.chest_state(a, 0), Some(ChestState::Missing));
    assert!(!game.source_state(game.scene_sources(a).unwrap()[0]).unwrap().enabled);
    assert!(game.is_alive(replacement));
    assert!(game.evict_scene(a));
    assert_eq!(game.chest_state(a, 0), None);
    assert_eq!(game.chest_state(b, 0), Some(ChestState::Locked));
}

#[test]
fn refused_chests_preserve_live_state_and_pending_work() {
    let mut game = Game::from_scene(&scene()).unwrap();
    step(&mut game, false, true);
    assert!(game.request_spawn(Vec2::new(20.0, 20.0), ActorDefinition::passive_hostile()).is_some());
    let before = observed(&game);
    let mut invalid = Vec::new();
    for body in [0, 99] { let mut s = scene(); s.chests[0].body = body; invalid.push(s); }
    let mut s = scene(); s.chests.push(s.chests[0]); invalid.push(s);
    let mut s = scene(); s.engine.bodies[1].what.physical = arpg_sim::Template::BLOCK.character(); invalid.push(s);
    let mut s = scene(); s.doors.push(DoorSpec { body: 1, panel: arpg_game::DoorPanel::default() }); invalid.push(s);
    let mut s = scene(); s.encounters[0].on_success = vec![Action::UnlockChest(99)]; invalid.push(s);
    let mut s = scene(); s.encounters[0].on_success.push(Action::UnlockChest(0)); invalid.push(s);
    let mut s = scene(); s.engine.sources.push(s.engine.sources[0].clone());
    let mut other = s.encounters[0].clone(); other.name = "Other".into();
    let EncounterObjective::Defeat(objective) = &mut other.objective; objective.sources = vec![1];
    s.encounters.push(other); invalid.push(s);
    for s in invalid {
        assert!(game.load_scene(&s).is_err()); assert_eq!(observed(&game), before);
        assert!(game.start_scene(&s).is_err()); assert_eq!(observed(&game), before);
    }
    step(&mut game, false, false);
    assert_eq!(game.enemy_count(), 2);
}

#[test]
fn capacity_is_global_and_released_on_eviction() {
    let mut s = scene(); s.encounters.clear(); s.engine.sources.clear();
    let mut game = Game::empty();
    let mut owners = Vec::new();
    for _ in 0..arpg_game::MAX_CHESTS { owners.push(game.load_scene(&s).unwrap()); }
    let before = observed(&game);
    assert!(game.load_scene(&s).is_err()); assert_eq!(observed(&game), before);
    assert!(game.evict_scene(owners[0]));
    assert!(game.load_scene(&s).is_ok());
}

#[test]
fn a_chest_and_a_physical_door_have_different_simulation_and_game_hashes() {
    let mut chest = scene(); chest.encounters.clear();
    let mut door = chest.clone(); door.chests.clear(); door.doors = vec![DoorSpec { body: 1, panel: arpg_game::DoorPanel::default() }];
    door.engine.bodies[1].what.physical = arpg_sim::Template::BODY;
    let a = Game::from_scene(&chest).unwrap();
    let b = Game::from_scene(&door).unwrap();
    assert_ne!(a.engine_hash(), b.engine_hash());
    assert_ne!(a.hash(), b.hash());
}
