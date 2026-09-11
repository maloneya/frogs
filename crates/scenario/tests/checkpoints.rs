//! Exercise the public CLI: a failed temporal assertion must exit nonzero even
//! when the final state is correct. These fixtures stay out of scenarios/ since
//! some deliberately fail; the normal scenario gate must remain all green.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn out_of_order_loads_cannot_validate_absence_against_a_different_scene() {
    let fixture = Fixture::new(r#"(
        scenes: [
            (at: 1, action: Load(Inline((name: "source", sources: [(pos: (20.0, 0.0))])))),
            (at: 0, action: Load(Inline((name: "empty")))),
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
fn scene_batches_cannot_bind_identities_from_a_truncated_trace() {
    // Evict before the step, so this tests the observation boundary without
    // asking the quadratic crowd solver to simulate a large coincident crowd.
    let bodies = "(pos: (20.0, 0.0)),".repeat(17_000);
    let fixture = Fixture::new(&format!(
        "(scenes: [(at: 0, action: Load(Inline((name: \"large\", bodies: [{bodies}])))),
                    (at: 0, action: Evict(0))], budget: (ticks: 1))"
    ));
    let (output, text) = fixture.run(&[]);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("placement trace"), "{text}");
}

#[test]
fn scene_commands_and_content_fail_loudly_even_when_blessing() {
    for source in [
        r#"(setup: (scenes: [File("missing.ron")]), budget: (ticks: 1))"#,
        r#"(setup: (scenes: [Inline((name: "bad", bodies: [(pos: (NaN, 0.0))]))]), budget: (ticks: 1))"#,
        r#"(setup: (enemies: 1, scenes: [Inline((name: "mixed"))]), budget: (ticks: 1))"#,
        r#"(scenes: [(at: 1, action: Load(Inline((name: "late"))))], budget: (ticks: 1))"#,
        r#"(scenes: [(at: 0, action: Evict(0))], budget: (ticks: 1))"#,
        r#"(setup: (scenes: [Inline((name: "typo", bodys: []))]), budget: (ticks: 1))"#,
    ] {
        let fixture = Fixture::new(source);
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "invalid scene passed: {source}\n{text}");
        }
    }
}

#[test]
fn recovery_commands_reject_invalid_values_and_unreachable_ticks() {
    for command in [
        "(at: 0, recovery: 0)",
        "(at: 0, recovery: 4294967295)",
        "(at: 0, recovery: -1)",
        "(at: 0, recovery: 1.5)",
        "(at: 1, recovery: 1)",
    ] {
        let fixture = Fixture::new(&format!(
            "(attack_recovery: [{command}], budget: (ticks: 1), expect: ())"
        ));
        let (output, text) = fixture.run(&[]);
        assert!(!output.status.success(), "invalid command {command} passed: {text}");
    }
}

#[test]
fn attack_profile_commands_reject_unknown_profiles_and_unreachable_ticks() {
    for command in [
        "(at: 0, profile: Typo)",
        "(at: 0, profile: 1)",
        "(at: 1, profile: Sweep)",
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
    // East then west: the final position is zero. A checker using final state
    // for the intermediate assertion would incorrectly accept both.
    let fixture = Fixture::new(
        r#"(
        inputs: [(at: 0, ticks: 1, dir: (1.0, 0.0)), (at: 1, ticks: 1, dir: (-1.0, 0.0))],
        checkpoints: [(at: 0, expect: (player_pos: (x: 0.0, z: 0.0, tol: 0.00001)))],
        budget: (ticks: 2),
        expect: (player_pos: (x: 0.0, z: 0.0, tol: 0.00001)),
    )"#,
    );
    for flags in [&[][..], &["--bless"][..]] {
        let (output, text) = fixture.run(flags);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains("checkpoint[0] after tick 0: player_pos"), "{text}");
        assert!(text.contains("actual:   (0.1500, 0.0000)"), "{text}");
    }
}

#[test]
fn checkpoints_observe_new_spawns_at_both_budget_edges_in_any_order() {
    // First seeker step is 3.5/60, and each checkpoint observes the step just
    // completed. Also checks that new placement IDs are registered before it.
    let fixture = Fixture::new(
        r#"(
        spawns: [(at: 0, pos: (10.0, 0.0), what: (seeks: true))],
        checkpoints: [
            (at: 1, expect: (bodies: [(nth: 0, alive: true,
                pos: (x: 9.8833333, z: 0.0, tol: 0.00001))])),
            (at: 0, expect: (enemy_count: 1, seekers: 1, bodies: [(nth: 0, seeking: true,
                pos: (x: 9.9416667, z: 0.0, tol: 0.00001),
                velocity: (x: 0.0, z: 0.0, tol: 0.0))])),
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
            (at: 0, expect: (seekers: 1)),
        ],
        budget: (ticks: 1),
    )"#,
    );
    let (output, text) = fixture.run(&[]);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("checkpoint[0] after tick 0: enemy_count"), "{text}");
    assert!(text.contains("checkpoint[1] after tick 0: seekers"), "{text}");
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
    assert!(text.contains("golden traces belong in the final expect.trace"), "{text}");
    assert!(!fixture.0.join("ignored.trace").exists());
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
        bodies: [(pos: (1.0, 0.0), what: (fixed: true, interactable: true))],
        sources: [(pos: (20.0, 0.0), enabled: false)])"#;
    for connection in [
        "[(body: 1, source: 0)]", "[(body: 0, source: 1)]",
        "[(body: 0, source: 0), (body: 0, source: 0)]",
        "[(body: 0, soruce: 0)]",
    ] {
        let fixture = Fixture::new(&format!(
            "(setup: (scenes: [Gameplay(({scene}, source_controls: {connection}))]), budget: (ticks: 2))"
        ));
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "{connection}: {text}");
        }
    }
    for fields in [
        "expect: (controls: [(scene: 0, control: 1, state: None)])",
        "expect: (controls: [(scene: 1, control: 0, state: None)])",
        "source_switches: [(at: 0, scene: 0, source: 1, enabled: true)]",
        "remove_sources: [(at: 0, scene: 1, source: 0)]",
        "despawns: [(at: 0, body: 99)]",
        "checkpoints: [(at: 0, expect: (controls: [(scene: 0, control: 0, state: Some((phase: Started, source: Some((enabled: true, countdown: 0, emitted: 1)))))]))], interactions: [0]",
    ] {
        let fixture = Fixture::new(&format!(
            "(setup: (scenes: [Gameplay(({scene}, source_controls: [(body: 0, source: 0)]))]), budget: (ticks: 2), {fields})"
        ));
        for flags in [&[][..], &["--bless"][..]] {
            let (output, text) = fixture.run(flags);
            assert_eq!(output.status.code(), Some(1), "{fields}: {text}");
        }
    }
}
