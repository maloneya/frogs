//! Guards the simulation's dependency graph, as an **allowlist**.
//!
//! A denylist was here first and it failed open: it named `arpg-gfx`, `wgpu` and
//! `winit`, so it caught exactly the three mistakes someone had already thought
//! of. Adding `bevy`, `hecs` or `rapier` to this crate sailed straight through —
//! and "do not introduce a game engine or an off-the-shelf ECS" is the loudest
//! rule the project has, because writing those layers is the entire point.
//!
//! An allowlist fails closed. Anything not named below stops the build,
//! including the dependency nobody predicted.

const CRATE: &str = "arpg-sim";
const FILE: &str = "crates/sim/build.rs";

const REASON: &str = "sim must stay free of the graphics stack and of the window system, \
                      because that is what keeps simulation tests runnable without a GPU; \
                      and the engine layers are hand-written on purpose, so an engine or \
                      an off-the-shelf ECS defeats the exercise.";

/// The complete set. `arpg-core` for shared vocabulary, `glam` for maths.
const ALLOWED: &[&str] = &["arpg-core", "glam"];

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
