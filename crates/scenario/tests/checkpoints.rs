//! Exercise the public CLI: a failed temporal assertion must exit nonzero even
//! when the final state is correct. These fixtures stay out of scenarios/ since
//! some deliberately fail; the normal scenario gate must remain all green.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn torch_observations_reject_nonexistent_indices_and_wrong_checkpoints() {
    for expect in [
        "torches: [(scene: 0, torch: 1, state: None)]",
        "torch_count: 0",
    ] {
        let fixture = Fixture::new(&format!(r#"(
            setup: (scenes: [(engine: (name: "lit"), torches: [
                (position: (0.0, 2.0, 0.0), colour: (1.0, 0.5, 0.1), intensity: 10.0, range: 8.0)
            ])]),
            checkpoints: [(at: 0, expect: ({expect}))],
            budget: (ticks: 2), expect: (torch_count: 1),
        )"#));
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "bad torch observation passed: {text}");
            assert!(text.contains("torch"), "{text}");
        }
    }
}

#[test]
fn out_of_order_loads_cannot_validate_absence_against_a_different_scene() {
    let fixture = Fixture::new(r#"(
        scenes: [
            (at: 1, action: Load((engine: (name: "source", sources: [(pos: (20.0, 0.0))])))),
            (at: 0, action: Load((engine: (name: "empty")))),
        ],
        budget: (ticks: 2),
        expect: (source_states: [(scene: 0, source: 0, state: None)]),
    )"#);
    for flags in [&[][..], &["--bless"][..]] {
        let (output, text) = fixture.run(flags);
        assert_eq!(output.status.code(), Some(1), "a nonexistent source passed: {text}");
        assert!(text.contains("source_states"), "{text}");
    }
}

#[test]
fn source_commands_and_assertions_fail_closed_even_when_blessing() {
    for fields in [
        "source_switches: [(at: 2, source: 0, enabled: true)]",
        "source_switches: [(at: 0, source: 1, enabled: true)]",
        "source_switches: [(at: 0, source: 0, enabled: 1)]",
        "remove_sources: [(at: 0, source: 0)], source_switches: [(at: 1, source: 0, enabled: true)]",
        "expect: (source_states: [(source: 1, state: None)])",
        "checkpoints: [(at: 0, expect: (source_states: [(source: 1, state: None)]))]",
        "expect: (source_states: [(source: 0, state: Some((enabled: false, countdown: 0, emitted: 0, typo: 0)))])",
    ] {
        let fixture = Fixture::new(&format!(
            "(setup: (sources: [(pos: (20.0, 0.0), enabled: false)]), budget: (ticks: 2), {fields})"
        ));
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "invalid source operation passed: {fields}\n{text}");
        }
    }
}

#[test]
fn source_checkpoint_failure_survives_a_correct_final_state() {
    let fixture = Fixture::new(
        r#"(
        setup: (sources: [(pos: (20.0, 0.0), enabled: false)]),
        source_switches: [(at: 1, source: 0, enabled: true)],
        checkpoints: [(at: 0, expect: (source_states: [
            (source: 0, state: Some((enabled: true, countdown: 0, emitted: 1)))
        ]))],
        budget: (ticks: 2),
        expect: (source_states: [
            (source: 0, state: Some((enabled: true, countdown: 0, emitted: 1)))
        ]),
    )"#,
    );
    for flags in [&[][..], &["--bless"][..]] {
        let (output, text) = fixture.run(flags);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains("checkpoint[0] after tick 0: source_states[0]"), "{text}");
    }
}

#[test]
fn scene_batches_bind_identities_even_when_diagnostic_history_overflows() {
    // Two load/evict batches produce over 65,536 placement/removal events.
    // Evict before stepping so no large coincident crowd reaches the solver.
    let bodies = "(pos: (20.0, 0.0)),".repeat(17_000);
    let fixture = Fixture::new(&format!(
        r#"(scenes: [
            (at: 0, action: Load((engine: (name: "large", bodies: [{bodies}])))),
            (at: 0, action: Evict(0)),
            (at: 0, action: Load((engine: (name: "large again", bodies: [{bodies}])))),
            (at: 0, action: Evict(1)),
        ], spawns: [(at: 0, pos: (40.0, 0.0))], budget: (ticks: 1),
        expect: (bodies: [(nth: 0, alive: false), (nth: 16999, alive: false),
            (nth: 17000, alive: false), (nth: 33999, alive: false),
            (nth: 34000, alive: true)]))"#
    ));
    let (output, text) = fixture.run(&[]);
    assert!(output.status.success(), "{text}");
}

#[test]
fn scene_commands_and_content_fail_loudly_even_when_blessing() {
    for source in [
        r#"(setup: (scenes: [File("missing.ron")]), budget: (ticks: 1))"#,
        r#"(setup: (scenes: [(engine: (name: "bad", bodies: [(pos: (NaN, 0.0))]))]), budget: (ticks: 1))"#,
        r#"(setup: (enemies: 1), budget: (ticks: 1))"#,
        r#"(scenes: [(at: 1, action: Load((engine: (name: "late"))))], budget: (ticks: 1))"#,
        r#"(scenes: [(at: 0, action: Evict(0))], budget: (ticks: 1))"#,
        r#"(setup: (scenes: [(engine: (name: "typo", bodys: []))]), budget: (ticks: 1))"#,
    ] {
        let fixture = Fixture::new(source);
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "invalid scene passed: {source}\n{text}");
        }
    }
}

#[test]
fn scenarios_reject_external_scene_files_in_setup_and_timeline() {
    for setup in [
        r#"setup: (scenes: [File("scene.ron")])"#,
        r#"scenes: [(at: 0, action: Load(File("scene.ron")))]"#,
    ] {
        let fixture = Fixture::new(&format!("({setup}, budget: (ticks: 1))"));
        std::fs::write(fixture.0.join("scene.ron"), "(scene: (engine: (name: \"External\")))").unwrap();
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "external scene was accepted: {text}");
            assert!(text.contains("parse"), "external scenes must be rejected by the schema: {text}");
        }
    }
}

#[test]
fn removed_recovery_edit_surface_is_rejected_by_the_schema() {
    for text in [
        "(attack_recovery: [(at: 0, recovery: 1)], budget: (ticks: 1))",
        "(budget: (ticks: 1), expect: (recovery_ticks: 1))",
        "(budget: (ticks: 1), expect: (swing_recovery_ticks: 1))",
    ] {
        let fixture = Fixture::new(text);
        let (output, text) = fixture.run(&[]);
        assert!(!output.status.success(), "removed recovery surface passed: {text}");
        assert!(text.contains("parse"));
    }
}

#[test]
fn attack_profile_commands_reject_unknown_profiles_and_unreachable_ticks() {
    for command in [
        "(at: 0, profile: Typo)",
        "(at: 0, profile: Basic)",
        "(at: 0, profile: Thrust)",
        "(at: 0, profile: Sweep)",
        "(at: 0, profile: HeavySweep)",
        "(at: 0, profile: CrowdBreaker)",
        "(at: 0, profile: 1)",
        "(at: 1, profile: Slam)",
    ] {
        let fixture = Fixture::new(&format!(
            "(attack_profiles: [{command}], budget: (ticks: 1), expect: ())"
        ));
        let (output, text) = fixture.run(&[]);
        assert!(!output.status.success(), "invalid command {command} passed: {text}");
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(source: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "arpg-checkpoints-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).expect("create test-owned directory");
        std::fs::write(directory.join("test.ron"), source).expect("write fixture");
        Self(directory)
    }

    fn run(&self, flags: &[&str]) -> (Output, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_scenario"))
            .args(flags)
            .arg(self.0.join("test.ron"))
            .output()
            .expect("run the headless scenario CLI");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        (output, text)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove test-owned directory");
    }
}

#[test]
fn a_transient_failure_survives_recovery_and_blessing() {
    // East adds 1m/s; reversing after drag gives -.1m/s on tick 1. Changes ramp
    // in over four substeps, so tick 0 covers 5/8 of 1/60m and the final x is
    // (.625 + (1 - 5/8 1.1))/60 = .015625m. A checker using final state for the
    // intermediate assertion would incorrectly accept both.
    let fixture = Fixture::new(
        r#"(
        inputs: [(at: 0, ticks: 1, dir: (1.0, 0.0)), (at: 1, ticks: 1, dir: (-1.0, 0.0))],
        checkpoints: [(at: 0, expect: (player_pos: (x: 0.015625, z: 0.0, tol: 0.00001)))],
        budget: (ticks: 2),
        expect: (player_pos: (x: 0.015625, z: 0.0, tol: 0.00001)),
    )"#,
    );
    for flags in [&[][..], &["--bless"][..]] {
        let (output, text) = fixture.run(flags);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains("checkpoint[0] after tick 0: player_pos"), "{text}");
        assert!(text.contains("actual:   (0.0104, 0.0000)"), "{text}");
    }
}

#[test]
fn checkpoints_observe_new_spawns_at_both_budget_edges_in_any_order() {
    // The movement action starts at .5 m/s, then reaches .95 m/s; each change ramps in over four
    // substeps (5/8 of it moves the body that tick). Each checkpoint observes the step just
    // completed. Also checks that new placement IDs are registered before it.
    let fixture = Fixture::new(
        r#"(
        spawns: [(at: 0, pos: (10.0, 0.0), what: (role: Hostile, physical: (character: true, health: Some(3)), actions: [Move(())], behavior: Action(0)))],
        checkpoints: [
            (at: 1, expect: (bodies: [(nth: 0, alive: true,
                pos: (x: 9.9817708, z: 0.0, tol: 0.00001))])),
            (at: 0, expect: (enemy_count: 1, bodies: [(nth: 0, role: Hostile, action: Move,
                pos: (x: 9.9947917, z: 0.0, tol: 0.00001),
                velocity: (x: -0.5, z: 0.0, tol: 0.0))])),
            (at: 0, expect: (player_pos: (x: 0.0, z: 0.0, tol: 0.0))),
        ],
        budget: (ticks: 2), expect: (enemy_count: 1),
    )"#,
    );
    let (output, text) = fixture.run(&[]);
    assert!(output.status.success(), "{text}");
}

#[test]
fn every_duplicate_checkpoint_reports_its_own_failure() {
    let fixture = Fixture::new(
        r#"(
        checkpoints: [
            (at: 0, expect: (enemy_count: 1)),
            (at: 0, expect: (sources: 1)),
        ],
        budget: (ticks: 1),
    )"#,
    );
    let (output, text) = fixture.run(&[]);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("checkpoint[0] after tick 0: enemy_count"), "{text}");
    assert!(text.contains("checkpoint[1] after tick 0: sources"), "{text}");
}

#[test]
fn unreachable_checkpoints_are_rejected_including_zero_budget_and_overflow() {
    for (budget, at) in [(0, 0), (2, 2), (2, u64::MAX)] {
        let fixture = Fixture::new(&format!(
            "(checkpoints: [(at: {at}, expect: (enemy_count: 0))], budget: (ticks: {budget}))",
        ));
        let (output, text) = fixture.run(&[]);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains(&format!("tick {at} will not run")), "{text}");
    }
}

#[test]
fn checkpoint_traces_are_rejected_instead_of_silently_ignored() {
    let fixture = Fixture::new(
        r#"(
        checkpoints: [(at: 0, expect: (trace: "ignored.trace"))], budget: (ticks: 1),
    )"#,
    );
    let (output, text) = fixture.run(&["--bless"]);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("transition evidence belongs in final expect.events, expect.gameplay_events or expect.trace"), "{text}");
    assert!(!fixture.0.join("ignored.trace").exists());
}

#[test]
fn checkpoint_event_contracts_are_rejected_instead_of_ignored() {
    for evidence in [
        "events: (kinds: [Damaged], sequence: [])",
        "gameplay_events: (kinds: [Swung], sequence: [])",
    ] {
        let fixture = Fixture::new(&format!("(checkpoints: [(at: 0, expect: ({evidence}))], budget: (ticks: 1))"));
        let (output, text) = fixture.run(&["--bless"]);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains("transition evidence belongs in final"), "{text}");
    }
}

#[test]
fn checkpoint_fields_are_strict_and_final_expectations_still_run() {
    let malformed = Fixture::new(
        r#"(
        checkpoints: [(at: 0, expect: (hitbxo: true))], budget: (ticks: 1),
    )"#,
    );
    let (output, text) = malformed.run(&[]);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("hitbxo"), "{text}");

    let wrong_final = Fixture::new(
        r#"(
        checkpoints: [(at: 0, expect: (enemy_count: 0))], budget: (ticks: 1),
        expect: (enemy_count: 1),
    )"#,
    );
    let (output, text) = wrong_final.run(&[]);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("enemy_count"), "{text}");
    assert!(!text.contains("checkpoint["), "{text}");
}

#[test]
fn gameplay_content_and_relationship_assertions_fail_closed() {
    let scene = r#"engine: (name: "trial",
        bodies: [(pos: (1.0, 0.0), what: (role: Prop, physical: (fixed: true, interactable: true)))],
        sources: [(pos: (20.0, 0.0), enabled: false)])"#;
    for fields in [
        "source_switches: [(at: 0, scene: 0, source: 1, enabled: true)]",
        "remove_sources: [(at: 0, scene: 1, source: 0)]",
        "despawns: [(at: 0, body: 99)]",
    ] {
        let fixture = Fixture::new(&format!(
            "(setup: (scenes: [({scene})]), budget: (ticks: 2), {fields})"
        ));
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "{fields}: {text}");
        }
    }
}

#[test]
fn invalid_aim_cannot_reach_game_even_when_blessing() {
    for aim in ["(at: 0, dir: (0.0, 0.0))", "(at: 0, dir: (NaN, 1.0))", "(at: 0, dir: (inf, 1.0))", "(at: 2, dir: (1.0, 0.0))"] {
        let fixture = Fixture::new(&format!("(aims: [{aim}], budget: (ticks: 2))"));
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "invalid aim passed: {text}");
        }
    }
}

#[test]
fn encounter_admission_and_observation_errors_cannot_be_blessed() {
    let scene = r#"engine: (name: "trial", bodies: [
        (pos: (1.0,0.0), what: (role: Prop, physical: (fixed:true, interactable:true))),
        (pos: (0.0,3.0), what: (role: Prop, physical: (fixed:false)))
    ], sources: [(pos: (0.0,10.0), enabled:false)]),
    doors: [(body:1, panel:(width:2.0, thickness:0.18, closed_yaw:0.0, min_angle:-1.55, max_angle:1.55, latch_impulse:12.0))],
    encounters: [(name:"Trial", start:Activated(0),
        objective:Defeat((sources:[0], count:1, within_ticks:10)),
        on_success:[UnlockDoor(0)])]"#;
    for (content,expect) in [
        (scene.to_owned(), "encounters:[(scene:0, encounter:1, state:None)]"),
        (scene.to_owned(), "doors:[(scene:0, door:1, state:None)]"),
        (scene.to_owned(), "doors:[(scene:0, door:0, state:Some(Released))]"),
        (scene.replace("count:1", "count:0"), ""),
        (scene.replace("within_ticks:10", "within_ticks:0"), ""),
        (scene.replace("sources:[0]", "sources:[99]"), ""),
        (scene.replace("UnlockDoor(0)", "UnlockDoor(99)"), ""),
        (scene.replace("count:1", "count:1, typo:3"), ""),
    ] {
        let fixture=Fixture::new(&format!("(setup:(scenes:[({content})]), checkpoints:[(at:0,expect:({expect}))], budget:(ticks:2))"));
        for flags in [&[][..], &["--bless"][..]] {
            let (output,text)=fixture.run(flags);
            assert_eq!(output.status.code(),Some(1),"invalid encounter passed: {text}");
        }
    }
}

#[test]
fn timing_evidence_preserves_results_and_cannot_overwrite_existing_evidence() {
    let fixture = Fixture::new("(budget: (ticks: 2), expect: (enemy_count: 0))");
    let (plain, text) = fixture.run(&[]);
    assert!(plain.status.success(), "{text}");
    let path = fixture.0.join("timings.json");
    let (timed, timed_text) = fixture.run(&["--timings", path.to_str().unwrap()]);
    assert!(timed.status.success(), "{timed_text}");
    assert_eq!(timed_text, text, "timing must not change scenario or replay outcomes");
    let saved = std::fs::read_to_string(&path).unwrap();
    for field in ["first_simulation_ms", "first_hashing_ms", "first_assertions_ms", "replay_ms"] {
        assert!(saved.contains(field), "missing timing component {field}");
    }
    let (repeated, repeated_text) = fixture.run(&["--timings", path.to_str().unwrap()]);
    assert!(!repeated.status.success(), "existing evidence was overwritten: {repeated_text}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), saved);
}

fn refuses(source: &str, diagnostic: &str) {
    let fixture = Fixture::new(source);
    for flags in [&[][..], &["--bless"][..]] {
        let (output, text) = fixture.run(flags);
        assert_eq!(output.status.code(), Some(1), "invalid scenario passed or panicked: {source}\n{text}");
        assert!(text.contains(diagnostic), "missing {diagnostic}: {text}");
    }
}

#[test]
fn every_float_prediction_rejects_invalid_numbers_at_final_and_checkpoints() {
    let mut predictions = Vec::new();
    for number in ["NaN", "inf", "-inf"] {
        predictions.push(format!("facing: (value: {number}, tol: 0.0)"));
        for field in ["x", "z"] {
            let pair = if field == "x" { format!("x: {number}, z: 0.0") }
                else { format!("x: 0.0, z: {number}") };
            for name in ["player_pos", "player_velocity"] {
                predictions.push(format!("{name}: ({pair}, tol: 0.0)"));
            }
            for name in ["pos", "velocity"] {
                predictions.push(format!("bodies: [(nth: 0, {name}: ({pair}, tol: 0.0))]"));
            }
        }
    }
    for tolerance in ["NaN", "inf", "-inf", "-0.1"] {
        predictions.push(format!("facing: (value: 0.0, tol: {tolerance})"));
        for name in ["player_pos", "player_velocity"] {
            predictions.push(format!("{name}: (x: 0.0, z: 0.0, tol: {tolerance})"));
        }
        for name in ["pos", "velocity"] {
            predictions.push(format!("bodies: [(nth: 0, {name}: (x: 0.0, z: 0.0, tol: {tolerance}))]"));
        }
    }
    for prediction in predictions {
        for assertion in [format!("expect: ({prediction})"),
            format!("checkpoints: [(at: 0, expect: ({prediction}))]")] {
            refuses(&format!("(setup: (actions: [Place((20.0, 0.0))]), budget: (ticks: 1), {assertion})"), "could not parse");
        }
    }
}

#[test]
fn event_predictions_and_performance_limits_refuse_invalid_numbers() {
    for value in ["(NaN, 0.0)", "(0.0, inf)", "(-inf, 0.0)"] {
        refuses(&format!("(budget: (ticks: 1), expect: (events: (kinds: [Impulsed], sequence: [(0, Impulsed(id: (0, 0), source: None, value: {value}, tolerance: 0.0))])))"), "could not parse");
    }
    for tolerance in ["NaN", "inf", "-inf", "-0.1"] {
        refuses(&format!("(budget: (ticks: 1), expect: (events: (kinds: [Impulsed], sequence: [(0, Impulsed(id: (0, 0), source: None, value: (0.0, 0.0), tolerance: {tolerance}))])))"), "could not parse");
    }
    for limit in ["NaN", "inf", "-inf", "0.0", "-1.0"] {
        refuses(&format!("(budget: (ticks: 0, max_mean_step_micros: {limit}))"), "budget.max_mean_step_micros");
    }
}

#[test]
fn every_timeline_command_must_be_reachable() {
    for at in ["2", "18446744073709551615"] {
        for (name, command) in [
            ("attacks", at.to_owned()),
            ("interactions", at.to_owned()),
            ("spawns", format!("(at: {at}, pos: (20.0, 0.0))")),
            ("impulses", format!("(at: {at}, target: Player, value: (1.0, 0.0))")),
            ("aims", format!("(at: {at}, dir: (1.0, 0.0))")),
            ("attack_profiles", format!("(at: {at}, profile: Slam)")),
            ("scenes", format!("(at: {at}, action: Evict(0))")),
            ("despawns", format!("(at: {at}, body: 0)")),
            ("source_switches", format!("(at: {at}, source: 0, enabled: true)")),
            ("remove_sources", format!("(at: {at}, source: 0)")),
        ] {
            refuses(&format!("(setup: (sources: [(pos: (20.0, 0.0))]), budget: (ticks: 2), {name}: [{command}])"), name);
        }
    }
    refuses("(attacks: [0], budget: (ticks: 0))", "attacks");
    refuses("(budget: (ticks: 18446744073709551615))", "budget");
}

#[test]
fn movement_and_channel_spans_cannot_be_empty_truncated_or_overflowed() {
    for (at, ticks) in [(0, 0), (2, 1), (1, 2), (1, u64::MAX), (u64::MAX, 1)] {
        for name in ["inputs", "channels"] {
            let dir = if name == "inputs" { ", dir: (1.0, 0.0)" } else { "" };
            refuses(&format!("(budget: (ticks: 2), {name}: [(at: {at}, ticks: {ticks}{dir})])"), name);
        }
    }
    for dir in ["(NaN, 0.0)", "(0.0, inf)", "(-inf, 1.0)", "(3.4028235e38, 0.0)", "(1.0e-40, 0.0)"] {
        refuses(&format!("(budget: (ticks: 2), inputs: [(at: 0, ticks: 2, dir: {dir})])"), "inputs");
    }
}

#[test]
fn valid_numeric_and_timeline_boundaries_still_run() {
    // A setup-only observation is valid; zero is an exact tolerance.
    let fixture = Fixture::new("(budget: (ticks: 0), expect: (player_pos: (x: 0.0, z: 0.0, tol: 0.0), facing: (value: 0.0, tol: 0.0)))");
    let (output, text) = fixture.run(&[]);
    assert!(output.status.success(), "{text}");
    for dir in ["(1.0, 0.0)", "(2.0, 0.0)"] {
        // One span ends exactly at the budget; the last input wins the overlap.
        // Tick 1 brakes from 1m/s to rest; the ramped change covers
        // (1 - 5/8)/60m, after tick 0's .625/60m.
        let fixture = Fixture::new(&format!("(budget: (ticks: 2), inputs: [(at: 0, ticks: 2, dir: {dir}), (at: 1, ticks: 1, dir: (0.0, 0.0))], channels: [(at: 0, ticks: 2)], interactions: [1], spawns: [(at: 1, pos: (20.0, 0.0), what: (role: Hostile))], impulses: [(at: 1, target: Player, value: (0.0, 0.0))], attacks: [1], checkpoints: [(at: 0, expect: (player_pos: (x: 0.010416667, z: 0.0, tol: 0.00001)))], expect: (enemy_count: 1, player_pos: (x: 0.016666667, z: 0.0, tol: 0.00001)))"));
        let (output, text) = fixture.run(&[]);
        assert!(output.status.success(), "{dir}: {text}");
    }
}

#[test]
fn missing_setup_targets_and_dead_position_predictions_cannot_pass() {
    for actions in ["Despawn(0)", "Assign(body: 0, actor: ())",
        "Place((20.0, 0.0)), Despawn(0), Despawn(0)",
        "Place((20.0, 0.0)), Despawn(0), Assign(body: 0, actor: ())",
        "Place((20.0, 0.0)), Assign(body: 0, actor: (actions: [Wait(ticks: 0)], behavior: Action(0)))"] {
        refuses(&format!("(setup: (actions: [{actions}]), budget: (ticks: 0))"), "setup");
    }
    for assertion in ["expect: (bodies: [(nth: 0, alive: false, pos: (x: 20.0, z: 0.0, tol: 0.0))])",
        "checkpoints: [(at: 0, expect: (bodies: [(nth: 0, alive: false, pos: (x: 20.0, z: 0.0, tol: 0.0))]))]"] {
        refuses(&format!("(setup: (actions: [Place((20.0, 0.0)), Despawn(0)]), budget: (ticks: 1), {assertion})"), "bodies[0].pos");
    }
}

#[test]
fn behavior_edits_refuse_unreachable_invalid_and_stale_assignments() {
    for command in [
        "behaviors: [(at: 1, body: 0, actor: ())]",
        "behaviors: [(at: 0, body: 0, actor: (actions: [Wait(ticks: 0)], behavior: Action(0)))]",
        "behaviors: [(at: 0, body: 99, actor: ())]",
    ] {
        refuses(&format!("(setup: (actions: [Place((20.0, 0.0))]), budget: (ticks: 1), {command})"), "behaviors");
    }
    refuses("(setup: (actions: [Place((20.0, 0.0)), Despawn(0)]), behaviors: [(at: 0, body: 0, actor: ())], budget: (ticks: 1))", "behaviors");
}
