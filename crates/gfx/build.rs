//! Guards the renderer's dependency graph, as an **allowlist**.
//!
//! Note what is absent: `arpg-sim`. Cargo would accept that edge — gfx and sim
//! are siblings, not a cycle — so this file is what refuses it. But a denylist
//! naming only `arpg-sim` failed open on everything else, so the rule is stated
//! the other way round: these dependencies and no others.

const CRATE: &str = "arpg-gfx";
const FILE: &str = "crates/gfx/build.rs";

const REASON: &str = "gfx must never know what an enemy is. Its vocabulary is \
                      arpg_core::Instance — position, scale, colour. If the renderer needs \
                      something the simulation has, widen Instance; do not reach across.";

/// The complete set, dev-dependencies included.
const ALLOWED: &[&str] =
    &["arpg-core", "bytemuck", "glam", "log", "png", "wgpu", "winit", "pollster"];

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
