//! Editor metadata and removals preserve their authoritative scene relationships.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{
    ChestSpec, DefeatObjective, DoorSpec, EncounterAction as Action, EncounterObjective,
    EncounterSpec, EncounterStart, Game, GameScene,
};
use arpg_sim::{Accumulator, Condition, Dt};
use arpg_game::SourceSpec;
use arpg_game::Placed;
use arpg_content::{SceneDocument, SceneObjectKind as Kind, SceneObjectLabel, SceneObjectRef as Target};

fn target(kind: Kind, index: usize) -> Target { Target { kind, index } }
fn scene() -> SceneDocument {
    let mut scene = GameScene::default();
    scene.engine.bodies = (0..5).map(|i| Placed {
        pos: (i as f32 * 3.0, 0.0), what: arpg_game::ActorDefinition::prop(),
    }).collect();
    scene.engine.bodies[0].what = arpg_game::ActorDefinition::prop().interactive();
    scene.engine.sources = (0..3).map(|i| SourceSpec {
        pos: (i as f32 * 3.0, 8.0), radius: 0.0, every: 60,
        when: Condition::Always, what: arpg_game::ActorDefinition::passive_hostile(), enabled: false,
    }).collect();
    scene.doors = vec![DoorSpec { body: 1 }, DoorSpec { body: 2 }];
    scene.chests = vec![ChestSpec { body: 3 }, ChestSpec { body: 4 }];
    scene.encounters = vec![EncounterSpec {
        name: "Trial".into(), start: EncounterStart::Activated(0),
        objective: EncounterObjective::Defeat(DefeatObjective {
            sources: vec![1, 2], count: 2.try_into().unwrap(),
            within_ticks: 60.try_into().unwrap(),
        }),
        on_start: vec![Action::UnlockDoor(1)],
        on_success: vec![Action::UnlockChest(1)],
        on_failure: vec![Action::UnlockDoor(1), Action::UnlockChest(1)],
    }];
    SceneDocument { scene, labels: Vec::new() }
}
fn observed(game: &Game) -> (u64, String, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish(), game.render_trace_since(0))
}
fn step(game: &mut Game) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}

#[test]
fn names_are_optional_unique_validated_and_do_not_change_gameplay() {
    let mut authored = scene();
    let first = target(Kind::Body, 0);
    let second = target(Kind::Source, 0);
    authored.set_object_name(first, "北の祠").unwrap();
    authored.set_object_name(second, "North crowd").unwrap();
    assert_eq!(authored.object_name(first), Some("北の祠"));
    let original = authored.labels.clone();
    for name in ["North crowd", " padded", "padded ", "line\nbreak", &"x".repeat(65)] {
        assert!(authored.set_object_name(first, name).is_err());
        assert_eq!(authored.labels, original);
    }
    assert!(authored.set_object_name(target(Kind::Body, 99), "Missing").is_err());
    assert_eq!(authored.labels, original);
    let mut named = Game::from_scene(&authored.scene).unwrap();
    let mut unnamed = Game::from_scene(&scene().scene).unwrap();
    assert_eq!(observed(&named), observed(&unnamed));
    step(&mut named);
    step(&mut unnamed);
    assert_eq!(observed(&named), observed(&unnamed));
    named.restart().unwrap();
    unnamed.restart().unwrap();
    assert_eq!(observed(&named), observed(&unnamed));
    authored.set_object_name(first, "Renamed shrine").unwrap();
    authored.set_object_name(first, "").unwrap();
    assert_eq!(authored.object_name(first), None);
    assert_eq!(authored.object_name(second), Some("North crowd"));
}

#[test]
fn invalid_labels_are_refused_by_authoring_but_do_not_enter_game() {
    let authored = scene();
    let valid = SceneObjectLabel { target: target(Kind::Body, 0), name: "Shrine".into() };
    let cases = vec![
        vec![SceneObjectLabel { name: "".into(), ..valid.clone() }],
        vec![SceneObjectLabel { name: " padded ".into(), ..valid.clone() }],
        vec![SceneObjectLabel { name: "\n".into(), ..valid.clone() }],
        vec![SceneObjectLabel { name: "x".repeat(65), ..valid.clone() }],
        vec![SceneObjectLabel { target: target(Kind::Body, 99), ..valid.clone() }],
        vec![valid.clone(), SceneObjectLabel { name: "Duplicate target".into(), ..valid.clone() }],
        vec![valid.clone(), SceneObjectLabel { target: target(Kind::Body, 1), ..valid }],
    ];
    for labels in cases {
        let mut invalid = authored.clone(); invalid.labels = labels;
        assert!(invalid.validate_labels().is_err());
        assert_eq!(Game::from_scene(&invalid.scene).unwrap().hash(), Game::from_scene(&authored.scene).unwrap().hash());
    }
}

#[test]
fn component_removal_refuses_references_and_remaps_all_transitions() {
    let mut authored = scene();
    let before = Game::from_scene(&authored.scene).unwrap().hash();
    assert!(authored.scene.remove_door(1).is_err());
    assert!(authored.scene.remove_chest(1).is_err());
    assert!(authored.scene.remove_door(9).is_err());
    assert!(authored.scene.remove_chest(9).is_err());
    assert_eq!(Game::from_scene(&authored.scene).unwrap().hash(), before);
    authored.scene.remove_door(0).unwrap();
    authored.scene.remove_chest(0).unwrap();
    assert_eq!(authored.scene.engine.bodies.len(), 5, "removing a component retains its body");
    assert_eq!(authored.scene.doors[0].body, 2);
    assert_eq!(authored.scene.chests[0].body, 4);
    assert_eq!(authored.scene.encounters[0].on_start, vec![Action::UnlockDoor(0)]);
    assert_eq!(authored.scene.encounters[0].on_success, vec![Action::UnlockChest(0)]);
    assert_eq!(authored.scene.encounters[0].on_failure, vec![Action::UnlockDoor(0), Action::UnlockChest(0)]);
    Game::from_scene(&authored.scene).unwrap();
}

#[test]
fn deleting_objects_keeps_surviving_names_and_relationships_on_the_same_objects() {
    let mut authored = scene();
    for (kind, index, name) in [
        (Kind::Body, 0, "Shrine"), (Kind::Body, 1, "Discarded gate"),
        (Kind::Body, 2, "Exit"), (Kind::Source, 0, "Discarded source"),
        (Kind::Source, 1, "North"), (Kind::Source, 2, "South"),
    ] {
        authored.set_object_name(target(kind, index), name).unwrap();
    }
    let before = authored.labels.clone();
    assert!(authored.remove_body(0).is_err());
    assert!(authored.remove_body(1).is_err());
    assert!(authored.remove_source(1).is_err());
    assert_eq!(authored.labels, before);
    authored.scene.remove_door(0).unwrap();
    authored.remove_body(1).unwrap();
    authored.remove_source(0).unwrap();
    assert_eq!(authored.object_name(target(Kind::Body, 1)), Some("Exit"));
    assert_eq!(authored.scene.doors[0].body, 1);
    assert_eq!(authored.scene.chests[0].body, 2);
    assert_eq!(authored.scene.chests[1].body, 3);
    assert_eq!(authored.object_name(target(Kind::Source, 0)), Some("North"));
    assert_eq!(authored.object_name(target(Kind::Source, 1)), Some("South"));
    let EncounterObjective::Defeat(goal) = &authored.scene.encounters[0].objective;
    assert_eq!(goal.sources, vec![0, 1]);
    assert_eq!(authored.object_name(target(Kind::Body, 0)), Some("Shrine"));
    Game::from_scene(&authored.scene).unwrap();
}

#[test]
fn label_compaction_is_limited_to_the_removed_collection() {
    for kind in [Kind::Wall, Kind::Grid, Kind::Tree, Kind::Campfire] {
        // The helper runs after collection removal, so its job is metadata only.
        let mut authored = SceneDocument { labels: vec![
            SceneObjectLabel { target: target(kind, 0), name: "First".into() },
            SceneObjectLabel { target: target(kind, 1), name: "Removed".into() },
            SceneObjectLabel { target: target(kind, 2), name: "Last".into() },
            SceneObjectLabel { target: target(Kind::Body, 2), name: "Body".into() },
        ], ..SceneDocument::default() };
        authored.remove_object_label(target(kind, 1));
        assert_eq!(authored.labels.len(), 3);
        assert_eq!(authored.object_name(target(kind, 0)), Some("First"));
        assert_eq!(authored.object_name(target(kind, 1)), Some("Last"));
        assert_eq!(authored.object_name(target(Kind::Body, 2)), Some("Body"));
    }
}
