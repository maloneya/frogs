//! Agent-style JSON and native-style edits share one headless document contract.
use std::{io::Write, process::{Command, Stdio}};
use serde_json::{Value, json};

fn call(request: Value, success: bool) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_arpg-author"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(request.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.success(), success, "{}", String::from_utf8_lossy(&output.stdout));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], success);
    response
}

#[test]
fn agent_and_visual_edits_round_trip_into_the_same_playable_encounter() {
    let path = std::env::temp_dir().join(format!("arpg-cli-parity-{}.ron", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let new = call(json!({"action":"new"}), true);
    let authored = call(json!({"action":"batch", "document":new["editor"]["document"], "edits":[
        {"action":"place","kind":"switch","pos":[1.0,0.0]},
        {"action":"place","kind":"chest","pos":[4.0,0.0]},
        {"action":"add","kind":"source","pos":[0.0,4.0]},
        {"action":"name","target":{"kind":"body","index":0},"name":"Shrine"},
        {"action":"encounter","index":null,"value":{
            "name":"Headless trial","trigger":0,"sources":[0],"count":1,"within_ticks":10,
            "on_start":[],"on_success":[arpg_game::EncounterAction::UnlockChest(0)],"on_failure":[]}}
    ]}), true);
    let saved = call(json!({"action":"save","document":authored["editor"]["document"],"path":path}), true);
    let opened = call(json!({"action":"open","path":path}), true);
    assert_eq!(opened["editor"]["document"], saved["editor"]["document"]);
    // The native map sends this exact edit shape. Both clients keep the rest opaque.
    let moved = call(json!({"action":"edit","document":opened["editor"]["document"],
        "edit":{"action":"move","target":{"kind":"body","index":1},"pos":[5.0,0.0]}}), true);
    let mut invalid = moved["editor"]["document"].clone();
    invalid["scene"]["engine"]["bodies"][0]["what"]["interactable"] = json!(false);
    call(json!({"action":"validate","document":invalid}), false);
    let before = std::fs::read(&path).unwrap();
    call(json!({"action":"save","document":invalid,"path":path,"revision":saved["editor"]["revision"]}), false);
    call(json!({"action":"save","document":moved["editor"]["document"],"path":path,"revision":"stale"}), false);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    call(json!({"action":"save","document":moved["editor"]["document"],"path":path,"revision":saved["editor"]["revision"]}), true);
    let reopened = call(json!({"action":"open","path":path}), true);
    assert_eq!(reopened["editor"]["objects"][0]["label"], "Shrine");
    assert_eq!(reopened["editor"]["objects"][1]["x"], 5.0);
    let scene = arpg_content::load_scene(&path).unwrap();
    assert!(!serde_json::to_value(&scene).unwrap().as_object().unwrap().contains_key("labels"));
    let mut game = arpg_game::Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    // Activation is consumed next tick, then the dormant source starts producing.
    assert_eq!(game.enemy_count(), 0);
    for interact in [true, false] {
        let dt = arpg_sim::Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
        game.step(dt, arpg_core::Intent::new(arpg_core::MoveDir::NONE, arpg_core::AimDir::FORWARD, false).with_interact(interact));
    }
    assert_eq!(game.enemy_count(), 1);
    assert_eq!(game.encounter_state(owner, 0).unwrap().phase, arpg_game::EncounterPhase::Running);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn integer_fields_preserve_owner_precision_through_edit_inspection_and_disk() {
    let path = std::env::temp_dir().join(format!("arpg-cli-integers-{}.ron", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let new = call(json!({"action":"new"}), true);
    let mut result = call(json!({"action":"edit", "document":new["editor"]["document"],
        "edit":{"action":"add","kind":"source","pos":[0,4]}}), true);
    for (field, value) in [("every", json!(16_777_217_u32)), ("every", json!(u32::MAX)),
        ("condition", json!(1)), ("threshold", json!(usize::MAX))] {
        result = call(json!({"action":"edit", "document":result["editor"]["document"],
            "edit":{"action":"set","target":{"kind":"source","index":0},"field":field,"value":value}}), true);
        let projected = result["editor"]["objects"][0]["fields"].as_array().unwrap().iter().find(|f| f["key"] == field).unwrap();
        if field == "condition" { assert_eq!(projected["value"].as_f64(), value.as_f64()); }
        else { assert_eq!(projected["value"], value); assert_eq!(projected["integer"], true); }
    }
    let accepted = result["editor"]["document"].clone();
    let saved = call(json!({"action":"save","document":accepted,"path":path}), true);
    let opened = call(json!({"action":"open","path":path}), true);
    assert_eq!(opened["editor"]["document"], accepted);
    for invalid in [json!(u64::from(u32::MAX) + 1), json!(-1), json!(1.5)] {
        call(json!({"action":"edit","document":accepted,
            "edit":{"action":"set","target":{"kind":"source","index":0},"field":"every","value":invalid}}), false);
    }
    let mut invalid = accepted.clone();
    invalid["scene"]["engine"]["sources"][0]["radius"] = json!(-1);
    call(json!({"action":"batch","document":invalid,"edits":[]}), false);
    call(json!({"action":"save","document":invalid,"path":path,"revision":saved["editor"]["revision"]}), false);
    assert_eq!(call(json!({"action":"open","path":path}), true)["editor"]["document"], accepted);
    std::fs::remove_file(path).unwrap();
}
