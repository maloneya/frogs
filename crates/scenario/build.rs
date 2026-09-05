//! Guards the scenario runner's dependency graph, as an **allowlist**.
//!
//! The absence of `arpg-gfx` and `winit` here is the whole point of the crate.
//! A scenario runs with no GPU, no window and no display server, which is what
//! makes it fast enough — microseconds, not the 500ms a `hold d 500` costs
//! through the harness — to be the gate on every change rather than something
//! run occasionally. One graphics dependency and that stops being true, quietly
//! and all at once.
//!
//! An allowlist fails closed: anything not named below stops the build,
//! including the dependency nobody predicted.

const CRATE: &str = "scenario";
const FILE: &str = "crates/scenario/build.rs";

const REASON: &str = "a scenario must run headless, with no GPU, no window and no wall clock, \
                      because that is what makes it cheap enough to be the gate on every \
                      change; and it asserts what the simulation does, so it has no business \
                      naming a renderer or a window system.";

/// The complete set. `arpg-sim` is what is under test, `arpg-core` and `glam`
/// are its vocabulary, and `ron`/`serde` parse the scenario files — plumbing,
/// not an engine layer, so a crate is the right call there.
const ALLOWED: &[&str] = &["arpg-core", "arpg-sim", "glam", "ron", "serde"];


use std::collections::HashSet;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");

    let manifest = std::fs::read_to_string("Cargo.toml").expect("read own Cargo.toml");

    for name in declared_dependencies(&manifest) {
        if !ALLOWED.contains(&name.as_str()) {
            panic!(
                "\n\n  {CRATE} may not depend on {name}.\n\n  {REASON}\n\n  \
                 Permitted: {permitted}.\n\n  \
                 If this edge is genuinely wanted, the architecture changed and this \
                 allowlist should be widened deliberately — in {FILE} — rather than \
                 worked around.\n\n",
                permitted = ALLOWED.join(", "),
            );
        }
    }
}

/// Dependency names only: section-aware, so a crate merely *mentioned* in a
/// comment or in `[package]` does not trip the guard. Every `dependencies`
/// section is scanned, dev-dependencies included — a test-only edge into the
/// graphics stack would defeat the point just as thoroughly as a real one.
fn declared_dependencies(manifest: &str) -> HashSet<String> {
    let mut found = HashSet::new();
    let mut in_deps = false;

    for line in manifest.lines() {
        let line = line.split('#').next().unwrap_or("").trim();

        if line.starts_with('[') {
            in_deps = line.contains("dependencies");
            continue;
        }
        if !in_deps || line.is_empty() {
            continue;
        }

        if let Some(key) = line.split('=').next() {
            let key = key.trim().trim_matches('"');
            let name = key.split('.').next().unwrap_or(key).trim();
            if !name.is_empty() {
                found.insert(name.to_string());
            }
        }
    }
    found
}
