//! The scenario runner: the gate a change to simulation behaviour has to pass.
//!
//! ```sh
//! cargo run --quiet -p scenario -- scenarios/
//! echo $?          # 0 or 1 — this is the whole point
//! ```
//!
//! A scenario is setup, an input stream measured in ticks, a tick budget, and
//! assertions at checkpoints and over the state it ends in. It runs against
//! `sim` with no GPU, no window and no wall-clock pacing — which lets it be the
//! gate on *every* change rather than something run occasionally.
//!
//! **Why this exists rather than a careful look at the numbers.** Driving the
//! real game and reading `player_pos 3.183` back off the socket puts the
//! comparison inside the agent's head, where it cannot fail visibly and cannot
//! be re-run tomorrow. Writing the same prediction here costs the same
//! keystrokes and turns it into a process that exits nonzero. The `Stop` hook
//! runs this when a turn ends and blocks on failure, so a scenario that stops
//! passing stops the work.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod run;
mod spec;

#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the workspace denies these to keep the game's frame path off stdout; this \
              crate has no frame path and its report is the entire product. Confined to \
              one module so the exception is a single visible decision."
)]
mod report;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let bless = args.iter().any(|a| a == "--bless");
    args.retain(|a| a != "--bless");

    if args.is_empty() {
        report::usage();
        return ExitCode::FAILURE;
    }

    let mut files = Vec::new();
    for arg in &args {
        if let Err(e) = collect(Path::new(arg), &mut files) {
            report::error(&format!("{arg}: {e}"));
            return ExitCode::FAILURE;
        }
    }

    // Sorted, so the report reads the same on every machine and a diff of two
    // runs is a diff of the results rather than of the directory order.
    files.sort();

    if files.is_empty() {
        report::error(&format!("no .ron scenarios found in {}", args.join(", ")));
        return ExitCode::FAILURE;
    }

    let failed = files.iter().filter(|f| !run_one(f, bless)).count();
    report::summary(failed, files.len());

    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Gathers `.ron` files from a path that may be either a file or a directory.
///
/// Not recursive, deliberately: `scenarios/` is meant to stay flat and
/// readable. A tree of them would be a sign the assertions want grouping, which
/// is a change to the format rather than to the file walk.
fn collect(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let p = entry?.path();
            if p.extension().is_some_and(|e| e == "ron") {
                out.push(p);
            }
        }
    } else {
        out.push(path.to_path_buf());
    }
    Ok(())
}

/// Runs one file and reports it. Returns whether it passed.
fn run_one(path: &Path, bless: bool) -> bool {
    let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return fail_to_load(&name, "read", &e.to_string()),
    };

    // `implicit_some` so an optional assertion is written `contacts: 0` rather
    // than `contacts: Some(0)`. Every field of `expect` is optional, so without
    // it the noise would be on every line of every scenario, and a format
    // people find noisy is a format that gets fewer assertions written in it.
    //
    // A parse error is reported as an ordinary failure rather than a panic: one
    // malformed file must not hide the results of every other scenario in the
    // directory. RON's error names the line and column.
    let options =
        ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);

    let mut scenario: spec::Scenario = match options.from_str(&source) {
        Ok(s) => s,
        Err(e) => return fail_to_load(&name, "parse", &e.to_string()),
    };

    // Resolve once, before both runs. Disk latency/content edits cannot change
    // which scene the replay receives. Paths are relative to this scenario file.
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    for scene in &mut scenario.setup.scenes {
        if let Err(error) = scene.resolve(base) {
            return fail_to_load(&name, "load scene", &error.to_string());
        }
    }
    for command in &mut scenario.scenes {
        if let spec::SceneAction::Load(scene) = &mut command.action
            && let Err(error) = scene.resolve(base)
        {
            return fail_to_load(&name, "load scene", &error.to_string());
        }
    }

    let invalid = run::validate(&scenario);
    if !invalid.is_empty() {
        report::failed(&name, &invalid);
        return false;
    }

    let outcome = run::run(&scenario);
    let mut failures = run::check(&scenario, &outcome);
    failures.extend(run::check_replay(&scenario, &outcome));
    failures.extend(run::check_trace(&scenario, &outcome, path, bless));

    if failures.is_empty() {
        // The world's own count, not the setup's. They part company the moment
        // a scenario places or despawns bodies, and a summary line that says
        // "0 enemies" for a run built entirely out of placements is a line that
        // teaches the reader to distrust the report.
        report::passed(
            &name,
            scenario.budget.ticks,
            outcome.world.enemy_count(),
            &scenario.description,
        );
        true
    } else {
        report::failed(&name, &failures);
        false
    }
}

fn fail_to_load(name: &str, stage: &str, message: &str) -> bool {
    report::failed(
        name,
        &[run::Failure {
            what: format!("could not {stage} the scenario file"),
            expected: "a readable scenario".into(),
            actual: message.into(),
            note: String::new(),
        }],
    );
    false
}
