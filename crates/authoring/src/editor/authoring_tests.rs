//! Author through the editor boundary, persist, then exercise the resulting Game.
use super::*;
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{ChestState, DoorState, EncounterAction as Action, EncounterPhase, };
use arpg_content::SceneObjectLabel;
use arpg_sim::{Accumulator, AttackProfile, Dt};

fn target(kind: Kind, index: usize) -> Target { Target { kind, index } }
fn apply(scene: &mut SceneDocument, operation: Edit) {
    *scene = edit(scene.clone(), operation).unwrap().0;
}
fn set_field(scene: &mut SceneDocument, target: Target, field: &str, value: f32) {
    apply(scene, Edit::Set { target, field: field.into(), value: Number::from_f64(f64::from(value)).unwrap() });
}
fn name(scene: &mut SceneDocument, target: Target, name: &str) {
    apply(scene, Edit::Name { target, name: name.into() });
}
fn draft() -> EncounterDraft {
    EncounterDraft { name: "Shrine trial".into(), trigger: 0, sources: vec![0],
        count: 1.try_into().unwrap(), within_ticks: 25.try_into().unwrap(),
        on_start: vec![Action::UnlockDoor(0)], on_success: vec![Action::UnlockChest(0)],
        on_failure: vec![Action::UnlockDoor(1)] }
}
fn authored_trial() -> SceneDocument {
    let mut scene = SceneDocument::default();
    for (kind, pos, label) in [
        (PropKind::Switch, (1.0, 0.0), "Shrine"),
        (PropKind::Door, (4.0, 0.0), "Entrance"),
        (PropKind::Chest, (-4.0, 0.0), "Reward"),
        (PropKind::Door, (8.0, 0.0), "Escape"),
    ] {
        apply(&mut scene, Edit::Place { kind, pos });
        let index = scene.scene.engine.bodies.len() - 1;
        name(&mut scene, target(Kind::Body, index), label);
    }
    apply(&mut scene, Edit::Add { kind: Kind::Source, pos: (0.0, 4.0) });
    let source = target(Kind::Source, 0);
    apply(&mut scene, Edit::Actor { target: target(Kind::Source, 0), value: ActorDefinition::passive_hostile() });
    for (field, value) in [("radius", 0.0), ("every", 10_000.0), ("condition", 0.0)] {
        set_field(&mut scene, source, field, value);
    }
    name(&mut scene, source, "Trial enemies");
    apply(&mut scene, Edit::Encounter { index: None, value: draft() });
    scene
}
pub(super) fn composed_scene() -> SceneDocument {
    serde_json::from_value(json!({"scene": {
        "engine": {"name": "Authoring composition test", "bodies": [
            {"pos": [1.0, 0.0], "what": {"role": "Prop", "physical": {"fixed": true, "interactable": true}}},
            {"pos": [0.0, 3.0], "what": {"role": "Prop", "physical": {"fixed": true}}},
        ], "sources": [{"pos": [0.0, 8.0], "radius": 0.5, "every": 60, "enabled": false, "what": ActorDefinition::passive_hostile()}]},
        "chests": [{"body": 1}],
        "encounters": [{"name": "Test trial", "start": {"Activated": 0},
            "objective": {"Defeat": {"sources": [0], "count": 1, "within_ticks": 60}},
            "on_success": [{"UnlockChest": 0}]}],
        "trees": [
            {"kind": "Pine", "position": [12.0, 0.0], "yaw": 0.3, "scale": 0.9},
            {"kind": "Broadleaf", "position": [-12.0, 0.0], "yaw": -0.2, "scale": 1.1},
            {"kind": "Bare", "position": [0.0, -12.0], "yaw": 0.0, "scale": 1.0},
        ],
        "torches": [{"position": [2.0, 2.2, 0.0], "colour": [1.0, 0.65, 0.3], "intensity": 20.0, "range": 12.0}],
    }})).unwrap()
}

fn step(game: &mut Game, interact: bool, beam: bool) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)
        .with_interact(interact).with_attack_held(beam));
}

#[test]
fn editor_authored_encounter_runs_activation_success_failure_and_restart() {
    let scene = authored_trial();
    assert!(!scene.scene.engine.sources[0].enabled, "assigning a source disables it atomically");
    let mut game = Game::from_scene(&scene.scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let source = game.scene_sources(owner).unwrap()[0];
    let fresh = game.hash();
    game.set_attack_profile(AttackProfile::Disintegrate);
    // Prediction: tick 0 activates the shrine; tick 1 begins the encounter and
    // emits one stationary enemy. Beam pulses on active ticks 1, 13, 25 defeat
    // it on the last allowed tick, unlocking success rather than failure.
    step(&mut game, true, false);
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, EncounterPhase::Ready);
    assert_eq!(game.enemy_count(), 0);
    step(&mut game, false, true);
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, EncounterPhase::Running);
    assert_eq!(game.encounter_state(owner, 0).unwrap().remaining_ticks, 24);
    assert_eq!(game.door_state(owner, 0), Some(DoorState::Unlocked));
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Locked));
    assert_eq!(game.enemy_count(), 1);
    for _ in 2..25 { step(&mut game, false, true); }
    assert_eq!(game.encounter_state(owner, 0).unwrap().remaining_ticks, 1);
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, EncounterPhase::Running);
    step(&mut game, false, true);
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, EncounterPhase::Succeeded);
    assert_eq!(game.encounter_state(owner, 0).unwrap().defeated, 1);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Unlocked));
    assert_eq!(game.door_state(owner, 1), Some(DoorState::Locked));
    assert!(!game.source_state(source).unwrap().enabled);
    game.restart().unwrap();
    assert_eq!(game.hash(), fresh);
    step(&mut game, true, false);
    for _ in 0..25 { step(&mut game, false, false); }
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, EncounterPhase::Failed);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Locked));
    assert_eq!(game.door_state(owner, 1), Some(DoorState::Unlocked));
    assert!(!game.source_state(source).unwrap().enabled);
}

#[test]
fn encounter_edits_refuse_conflicting_ownership_and_remap_surviving_references() {
    let mut scene = authored_trial();
    let original = serde_json::to_value(&scene).unwrap();
    for invalid in [
        EncounterDraft { sources: vec![], ..draft() },
        EncounterDraft { sources: vec![0, 0], ..draft() },
        EncounterDraft { sources: vec![99], ..draft() },
        EncounterDraft { trigger: 1, ..draft() },
        EncounterDraft { on_success: vec![Action::UnlockChest(99)], ..draft() },
        EncounterDraft { on_success: vec![Action::UnlockChest(0), Action::UnlockChest(0)], ..draft() },
    ] {
        assert!(edit(scene.clone(), Edit::Encounter { index: Some(0), value: invalid }).is_err());
        assert_eq!(serde_json::to_value(&scene).unwrap(), original);
    }
    assert!(edit(scene.clone(), Edit::Encounter { index: None,
        value: EncounterDraft { name: "Other".into(), ..draft() } }).is_err());
    for target in [target(Kind::Body, 0), target(Kind::Body, 1), target(Kind::Body, 2), target(Kind::Source, 0)] {
        assert!(edit(scene.clone(), Edit::Delete { target }).is_err());
    }
    assert!(edit(scene.clone(), Edit::Set { target: target(Kind::Body, 1), field: "component".into(), value: Number::from_f64(2.0).unwrap() }).is_err());
    assert!(edit(scene.clone(), Edit::Set { target: target(Kind::Source, 0), field: "enabled".into(), value: Number::from_f64(1.0).unwrap() }).is_err());
    apply(&mut scene, Edit::Encounter { index: Some(0), value: EncounterDraft {
        on_start: vec![], on_success: vec![Action::UnlockDoor(1), Action::UnlockChest(0)], on_failure: vec![], ..draft()
    } });
    apply(&mut scene, Edit::Delete { target: target(Kind::Body, 1) });
    assert_eq!(scene.scene.encounters[0].on_success, vec![Action::UnlockDoor(0), Action::UnlockChest(0)]);
    assert_eq!(scene.scene.chests[0].body, 1);
    assert_eq!(scene.scene.doors[0].body, 2);
    assert_eq!(scene.object_name(target(Kind::Body, 1)), Some("Reward"));
    let projected = document(&scene, None, None, None).unwrap();
    assert_eq!(projected["rewards"][0]["body"], 2);
    assert_eq!(projected["rewards"][0]["label"], "Escape");
    apply(&mut scene, Edit::DeleteEncounter { index: 0 });
    assert!(!scene.scene.engine.sources[0].enabled, "removing a rule never silently starts its producers");
    apply(&mut scene, Edit::Delete { target: target(Kind::Source, 0) });
    apply(&mut scene, Edit::Delete { target: target(Kind::Body, 0) });
    assert_eq!(scene.scene.chests[0].body, 0);
    assert_eq!(scene.scene.doors[0].body, 1);
}

#[test]
fn actor_definitions_and_compositions_are_editable_on_every_producer() {
    for kind in [Kind::Body, Kind::Source, Kind::Grid] {
        let mut scene = SceneDocument::default();
        apply(&mut scene, Edit::Add { kind, pos: (8.0, 8.0) });
        let object = target(kind, 0);
        apply(&mut scene, Edit::Actor { target: object, value: ActorDefinition::civilian() });
        set_field(&mut scene, object, "action_count", 2.0);
        set_field(&mut scene, object, "action.0.kind", 0.0);
        set_field(&mut scene, object, "action.0.x", 12.0);
        set_field(&mut scene, object, "action.0.speed", 1.5);
        set_field(&mut scene, object, "action.1.ticks", 90.0);
        set_field(&mut scene, object, "behavior.kind", 2.0);
        set_field(&mut scene, object, "behavior.children", 2.0);
        set_field(&mut scene, object, "behavior.0.kind", 1.0);
        set_field(&mut scene, object, "behavior.1.kind", 1.0);
        set_field(&mut scene, object, "behavior.1.action", 2.0);
        let actor = match kind {
            Kind::Body => &scene.scene.engine.bodies[0].what,
            Kind::Source => &scene.scene.engine.sources[0].what,
            Kind::Grid => &scene.scene.engine.grids[0].what,
            _ => unreachable!(),
        };
        assert_eq!(actor.role, arpg_game::ActorRole::Civilian);
        assert_eq!(actor.actions[1], arpg_game::ActionDefinition::Wait { ticks: 90 });
        assert_eq!(actor.behavior, arpg_game::BehaviorNode::Sequence(vec![arpg_game::BehaviorNode::Action(0), arpg_game::BehaviorNode::Action(1)]));
        set_field(&mut scene, object, "action.0.order", 2.0);
        let actor = match kind {
            Kind::Body => &scene.scene.engine.bodies[0].what,
            Kind::Source => &scene.scene.engine.sources[0].what,
            Kind::Grid => &scene.scene.engine.grids[0].what,
            _ => unreachable!(),
        };
        assert_eq!(actor.behavior, arpg_game::BehaviorNode::Sequence(vec![arpg_game::BehaviorNode::Action(1), arpg_game::BehaviorNode::Action(0)]));
        set_field(&mut scene, object, "action.1.order", 1.0);
        let projection = document(&scene, Some(object), None, None).unwrap();
        for key in ["fixed", "character", "interactable", "actor_role", "action.0.speed", "behavior.1.action"] {
            assert!(projection["objects"][0]["fields"].as_array().unwrap().iter().any(|f| f["key"] == key));
        }
        for (field, value) in [("fixed", 1.0), ("action_count", 0.0), ("action_count", 1e9), ("behavior.children", 1e9), ("action.0.speed", -1.0)] {
            assert!(edit(scene.clone(), Edit::Set { target: object, field: field.into(), value: Number::from_f64(value).unwrap() }).is_err());
        }
    }
}

#[test]
fn components_and_actor_definitions_follow_edit_duplicate_and_delete_lifetimes() {
    let mut scene = SceneDocument::default();
    apply(&mut scene, Edit::Place { kind: PropKind::Prop, pos: (5.0, 5.0) });
    let body = target(Kind::Body, 0);
    set_field(&mut scene, body, "component", 1.0);
    apply(&mut scene, Edit::Duplicate { target: body, offset: (3.0, 0.0) });
    assert_eq!(scene.scene.doors.iter().map(|d| d.body).collect::<Vec<_>>(), vec![0, 1]);
    set_field(&mut scene, body, "component", 2.0);
    assert_eq!(scene.scene.doors[0].body, 1);
    assert_eq!(scene.scene.chests[0].body, 0);
    apply(&mut scene, Edit::Duplicate { target: body, offset: (0.0, 3.0) });
    assert_eq!(scene.scene.chests.iter().map(|c| c.body).collect::<Vec<_>>(), vec![0, 2]);
    set_field(&mut scene, body, "component", 0.0);
    apply(&mut scene, Edit::Actor { target: body, value: ActorDefinition::attacker() });
    apply(&mut scene, Edit::Duplicate { target: body, offset: (-2.0, 0.0) });
    assert_eq!(scene.scene.engine.bodies[0].what, scene.scene.engine.bodies[3].what);
    assert!(edit(scene.clone(), Edit::Set { target: target(Kind::Body, 3), field: "fixed".into(), value: Number::from_f64(1.0).unwrap() }).is_err());
    apply(&mut scene, Edit::Delete { target: body });
    assert_eq!(scene.scene.engine.bodies[2].what, ActorDefinition::attacker());
    assert_eq!(scene.scene.doors[0].body, 0);
    assert_eq!(scene.scene.chests[0].body, 1);
    apply(&mut scene, Edit::Delete { target: target(Kind::Body, 2) });
    apply(&mut scene, Edit::Delete { target: target(Kind::Body, 0) });
    assert!(scene.scene.doors.is_empty());
    assert_eq!(scene.scene.chests[0].body, 0);
}

#[test]
fn names_and_encounters_round_trip_and_refused_save_preserves_the_file() {
    let scene = authored_trial();
    let path = std::env::temp_dir().join(format!("arpg-editor-authoring-{}.ron", std::process::id()));
    let revision = arpg_content::save_scene_document(&path, &scene, None).unwrap();
    let (loaded, read_revision) = arpg_content::read_scene_document(&path).unwrap();
    assert_eq!(read_revision, revision);
    assert_eq!(serde_json::to_value(&loaded).unwrap(), serde_json::to_value(&scene).unwrap());
    assert_eq!(Game::from_scene(&loaded.scene).unwrap().hash(), Game::from_scene(&scene.scene).unwrap().hash());
    let original = std::fs::read(&path).unwrap();
    let mut invalid = loaded.clone();
    invalid.labels.push(SceneObjectLabel { target: target(Kind::Body, 99), name: "Missing".into() });
    assert!(arpg_content::save_scene_document(&path, &invalid, Some(&revision)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // JSON is the companion's opaque draft format; it must preserve labels too.
    let document = document(&loaded, None, None, None).unwrap();
    let copied: SceneDocument = serde_json::from_value(document["document"].clone()).unwrap();
    assert_eq!(copied.labels, loaded.labels);
    assert_eq!(document["objects"][0]["label"], "Shrine");
    assert_eq!(document["encounters"][0]["name"], "Shrine trial");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn test_composition_can_be_built_through_editor_operations_without_editing_ron() {
    let reference = composed_scene();
    let mut scene = SceneDocument::default();
    apply(&mut scene, Edit::Rename { name: reference.scene.engine.name.clone() });
    for body in &reference.scene.engine.bodies {
        let kind = if body.what.physical.interactable() { PropKind::Switch } else { PropKind::Chest };
        apply(&mut scene, Edit::Place { kind, pos: body.pos });
    }
    for (index, source) in reference.scene.engine.sources.iter().enumerate() {
        apply(&mut scene, Edit::Add { kind: Kind::Source, pos: source.pos });
        apply(&mut scene, Edit::Actor { target: target(Kind::Source, index), value: source.what.clone() });
        for (field, value) in [("radius", source.radius), ("every", source.every as f32), ("condition", 0.0)] {
            set_field(&mut scene, target(Kind::Source, index), field, value);
        }
    }
    for (index, tree) in reference.scene.trees.iter().enumerate() {
        apply(&mut scene, Edit::Add { kind: Kind::Tree, pos: tree.position().into() });
        let variant = match tree.kind() { TreeKind::Pine => 0.0, TreeKind::Broadleaf => 1.0, TreeKind::Bare => 2.0 };
        for (field, value) in [("kind", variant), ("yaw", tree.yaw().to_degrees()), ("scale", tree.scale())] {
            set_field(&mut scene, target(Kind::Tree, index), field, value);
        }
    }
    for (index, torch) in reference.scene.torches.iter().enumerate() {
        let light = torch.light();
        let target = target(Kind::Campfire, index);
        apply(&mut scene, Edit::Add { kind: Kind::Campfire, pos: (light.position().x, light.position().z) });
        apply(&mut scene, Edit::Colour { target, colour: light.colour().to_vec3().into() });
        for (field, value) in [("height", light.position().y), ("intensity", light.intensity()), ("range", light.range())] {
            set_field(&mut scene, target, field, value);
        }
    }
    let light = reference.scene.environment.light();
    for (field, value) in [("ambient_intensity", light.ambient_intensity()), ("directional_intensity", light.directional_intensity())] {
        apply(&mut scene, Edit::Lighting { field: field.into(), value });
    }
    for encounter in &reference.scene.encounters {
        let arpg_game::EncounterStart::Activated(trigger) = encounter.start;
        let arpg_game::EncounterObjective::Defeat(goal) = &encounter.objective;
        apply(&mut scene, Edit::Encounter { index: None, value: EncounterDraft {
            name: encounter.name.clone(), trigger, sources: goal.sources.clone(),
            count: goal.count, within_ticks: goal.within_ticks, on_start: encounter.on_start.clone(),
            on_success: encounter.on_success.clone(), on_failure: encounter.on_failure.clone(),
        } });
    }
    assert_eq!(serde_json::to_value(&scene.scene.engine).unwrap(), serde_json::to_value(&reference.scene.engine).unwrap());
    assert_eq!(serde_json::to_value(&scene.scene.encounters).unwrap(), serde_json::to_value(&reference.scene.encounters).unwrap());
    assert_eq!(serde_json::to_value(&scene.scene.chests).unwrap(), serde_json::to_value(&reference.scene.chests).unwrap());
    assert_eq!(serde_json::to_value(&scene.scene.torches).unwrap(), serde_json::to_value(&reference.scene.torches).unwrap());
    assert_eq!(scene.scene.environment, reference.scene.environment);
    assert_eq!(scene.scene.trees.len(), reference.scene.trees.len());
    for (actual, expected) in scene.scene.trees.iter().zip(&reference.scene.trees) {
        assert_eq!(actual.position(), expected.position());
        assert_eq!(actual.kind(), expected.kind());
        assert_eq!(actual.scale(), expected.scale());
        // Inspector rotation uses degrees; the owner stores radians.
        assert!((actual.yaw() - expected.yaw()).abs() < 0.000001);
    }
    let mut game = Game::from_scene(&scene.scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    step(&mut game, true, false);
    step(&mut game, false, false);
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, EncounterPhase::Running);
    assert_eq!(game.encounter_state(owner, 0).unwrap().target, 1);
    assert_eq!(game.enemy_count(), 1);
    assert_eq!(game.chest_state(owner, 0), Some(ChestState::Locked));
}

#[test]
fn a_civilian_routine_authored_in_the_inspector_moves_waits_and_restarts() {
    let mut scene = SceneDocument::default();
    let body = target(Kind::Body, 0);
    apply(&mut scene, Edit::Add { kind: Kind::Body, pos: (8.0, 0.0) });
    apply(&mut scene, Edit::Actor { target: body, value: ActorDefinition::civilian() });
    for (key, value) in [("action_count", 2.0), ("action.0.kind", 0.0), ("action.0.x", 10.0),
        ("action.0.stop_distance", 0.1), ("action.1.ticks", 30.0), ("behavior.kind", 2.0),
        ("behavior.children", 2.0), ("behavior.0.kind", 1.0), ("behavior.1.kind", 1.0), ("behavior.1.action", 2.0)] {
        set_field(&mut scene, body, key, value);
    }
    let mut game = Game::from_scene(&scene.scene).unwrap();
    let id = game.scene_bodies(game.scene_instances().next().unwrap().0).unwrap()[0];
    assert_eq!(game.health(id), None);
    assert_eq!(game.enemy_count(), 0);
    let initial = game.hash();
    step(&mut game, false, false);
    assert_eq!(game.action_status(id).unwrap().kind, arpg_game::ActionKind::Move);
    let mut reached_wait = false;
    for _ in 0..300 {
        step(&mut game, false, false);
        if game.action_status(id).unwrap().kind == arpg_game::ActionKind::Wait { reached_wait = true; break; }
    }
    assert!(reached_wait, "the authored movement completes and advances its sequence");
    for _ in 0..10 { step(&mut game, false, false); }
    assert_eq!(game.action_status(id).unwrap().kind, arpg_game::ActionKind::Wait);
    game.restart().unwrap();
    assert_eq!(game.hash(), initial);
}
