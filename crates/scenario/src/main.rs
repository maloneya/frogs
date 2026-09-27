//! The scenario runner: the gate a change to simulation behaviour has to pass.
//!
//! ```sh
//! cargo run --quiet -p scenario -- scenarios/
//! echo $?          # 0 or 1 — this is the whole point
//! ```
//!
//! A scenario is setup, an input stream measured in ticks, a tick budget, and
//! assertions at checkpoints and over the state it ends in. It runs against
//! `game` with no GPU, no window and no wall-clock pacing — which lets it be the
//! gate on *every* change rather than something run occasionally.
//!
//! Assertions make predictions repeatable and produce a nonzero exit on failure.
//! Run this gate explicitly unless the current tool environment has already run
//! it; Claude Code's Stop hook is one such environment.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod events;
mod run;
mod spec;
mod timing;

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

    let timings = if let Some(index) = args.iter().position(|arg| arg == "--timings") {
        args.remove(index);
        if index == args.len() {
            report::error("--timings requires an output path");
            return ExitCode::FAILURE;
        }
        Some(PathBuf::from(args.remove(index)))
    } else { None };
    if timings.as_ref().is_some_and(|path| path.exists()) {
        report::error("timings output already exists; choose a new path");
        return ExitCode::FAILURE;
    }

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

    files.dedup();
    let mut samples = Vec::new();
    let failed = files.iter().filter(|file| {
        let started = std::time::Instant::now();
        let mut sample = timing::Sample { file: file.display().to_string(), ..Default::default() };
        let passed = run_one(file, bless, &mut sample);
        sample.passed = passed;
        sample.total = started.elapsed();
        samples.push(sample);
        !passed
    }).count();
    if let Some(path) = timings
        && let Err(error) = timing::write(&path, &samples) {
        report::error(&format!("cannot write timings: {error}"));
        return ExitCode::FAILURE;
    }
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
fn run_one(path: &Path, bless: bool, sample: &mut timing::Sample) -> bool {
    let setup_started = std::time::Instant::now();
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

    let scenario: spec::Scenario = match options.from_str(&source) {
        Ok(s) => s,
        Err(e) => return fail_to_load(&name, "parse", &e.to_string()),
    };

    let invalid = run::validate(&scenario);
    if !invalid.is_empty() {
        report::failed(&name, &invalid);
        return false;
    }

    let setup = setup_started.elapsed();
    let outcome = run::run(&scenario);
    let assertions = std::time::Instant::now();
    let mut failures = run::check(&scenario, &outcome);
    let checking = assertions.elapsed();
    let replay = std::time::Instant::now();
    failures.extend(run::check_replay(&scenario, &outcome));
    sample.replay = replay.elapsed();
    let assertions = std::time::Instant::now();
    failures.extend(run::check_trace(&scenario, &outcome, path, bless));
    let final_assertions = checking + assertions.elapsed();

    sample.first = outcome.timing;
    sample.first.setup += setup;
    sample.first.assertions += final_assertions;
    if failures.is_empty() {
        // The world's own count, not the setup's. They part company the moment
        // a scenario places or despawns bodies, and a summary line that says
        // "0 enemies" for a run built entirely out of placements is a line that
        // teaches the reader to distrust the report.
        report::passed(
            &name,
            scenario.budget.ticks,
            outcome.game.enemy_count(),
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
