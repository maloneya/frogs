//! Everything this program prints, in one place.
//!
//! The module exists so that the workspace's `print_stdout` / `print_stderr`
//! denial can be lifted exactly once, with a reason, rather than at a dozen
//! call sites. That lint is there to keep the *game's* frame path off stdout;
//! this crate has no frame path, and its report is the entire product.
//!
//! Keeping the output here has a second benefit worth more than the first: the
//! failure message *is* the interface for anyone using this, agent or human. A
//! run that says "FAIL" and nothing else costs a round trip to find out what
//! happened, and round trips are the thing the whole scenario idea exists to
//! reduce.

use crate::run::Failure;

pub(crate) fn usage() {
    eprintln!("usage: scenario [--bless] <file.ron | directory>...");
    eprintln!();
    eprintln!("Runs each scenario headlessly against the simulation and exits 0 or 1.");
    eprintln!("Every scenario is also replayed and compared tick by tick, so a loss of");
    eprintln!("determinism fails even a scenario that asserts nothing about it.");
    eprintln!();
    eprintln!("--bless rewrites every golden trace file instead of comparing. Read the diff.");
}

pub(crate) fn error(message: &str) {
    eprintln!("error: {message}");
}

pub(crate) fn passed(name: &str, ticks: u64, enemies: usize, description: &str) {
    println!("PASS  {name:<34} {ticks:>5} tick(s)  {enemies:>5} enem(ies)");
    if !description.is_empty() {
        // Indented under the result, so a green run stays scannable while the
        // intent of each scenario is still there to read.
        println!("        {}", description.trim());
    }
}

pub(crate) fn failed(name: &str, failures: &[Failure]) {
    println!("FAIL  {name}");
    for f in failures {
        println!("        {}", f.what);
        println!("          expected: {}", f.expected);
        println!("          actual:   {}", f.actual);
        if !f.note.is_empty() {
            println!("          note:     {}", f.note);
        }
    }
}

pub(crate) fn summary(failed: usize, total: usize) {
    println!();
    if failed == 0 {
        println!("{total} scenario(s) passed");
    } else {
        println!("{failed} of {total} scenario(s) FAILED");
    }
}
