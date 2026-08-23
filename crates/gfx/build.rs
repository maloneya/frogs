//! Guards the one dependency edge that must never exist.
//!
//! Splitting into crates made `gfx` unable to *name* a simulation type without
//! declaring the dependency first — that is a real compile error (E0432), and it
//! is most of the value. But it is not the whole story: `arpg-gfx` and
//! `arpg-sim` are siblings, both depending only on `arpg-core`, so adding
//! `arpg-sim` to this crate's dependencies is not a cycle and Cargo accepts it
//! without complaint.
//!
//! This closes that. It runs on every build rather than only under `cargo test`,
//! so there is no path to a compiled binary that skips it, and the failure names
//! the rule rather than leaving the next person to infer it.

use std::collections::HashSet;

/// Crates this one must never depend on, and why.
const FORBIDDEN: &[(&str, &str)] = &[(
    "arpg-sim",
    "gfx must never know what an enemy is. Its vocabulary is arpg_core::Instance \
     — position, scale, colour. If the renderer needs something the simulation \
     has, widen Instance; do not reach across.",
)];

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");

    let manifest = std::fs::read_to_string("Cargo.toml").expect("read own Cargo.toml");
    let declared = declared_dependencies(&manifest);

    for (crate_name, reason) in FORBIDDEN {
        if declared.contains(*crate_name) {
            panic!(
                "\n\n  arpg-gfx must not depend on {crate_name}.\n\n  {reason}\n\n  \
                 If this edge is genuinely wanted, the architecture changed and \
                 this guard should be updated deliberately — in crates/gfx/build.rs \
                 — rather than worked around.\n\n"
            );
        }
    }
}

/// Dependency names only: section-aware, so a crate merely *mentioned* in a
/// comment or in `[package]` does not trip the guard.
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

        // `name = ...`, `name.workspace = true`, or `"name" = ...`
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
