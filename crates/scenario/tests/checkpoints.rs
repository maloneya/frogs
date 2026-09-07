//! Exercise the public CLI: a failed temporal assertion must exit nonzero even
//! when the final state is correct. These fixtures stay out of scenarios/ since
//! some deliberately fail; the normal scenario gate must remain all green.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

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
