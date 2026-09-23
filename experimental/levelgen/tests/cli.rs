//! End-to-end terminal contracts, including rejection without silent success.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_levelgen"))
        .args(args)
        .output()
        .expect("launch levelgen")
}

#[test]
fn accepted_output_is_repeatable_and_stages_preserve_the_final_result() {
    let plain = run(&["--seed", "42"]);
    assert!(plain.status.success());
    assert_eq!(plain.stdout, run(&["--seed", "42"]).stdout);
    let stages = run(&["--seed", "42", "--stages"]);
    assert!(stages.status.success());
    let plain = String::from_utf8(plain.stdout).unwrap();
    let stages = String::from_utf8(stages.stdout).unwrap();
    assert!(plain.starts_with("ACCEPTED"));
    assert!(stages.contains("random cells"));
    assert!(stages.ends_with(plain.split_once('\n').unwrap().1));
}

#[test]
fn exhausted_budget_is_failure_even_though_a_diagnostic_map_is_printed() {
    let result = run(&["--open-min", "100", "--open-max", "100", "--attempts", "2"]);
    assert_eq!(result.status.code(), Some(1));
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.starts_with("REJECTED"));
    assert!(text.contains("attempt=2/2"));
    assert!(text.contains("FAIL: open area"));
}

#[test]
fn invalid_configuration_produces_no_map() {
    let result = run(&["--passage-width", "0"]);
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("passage width")
    );
}
