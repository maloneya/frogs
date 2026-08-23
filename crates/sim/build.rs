//! The mirror of `crates/gfx/build.rs`, guarding the other direction.
//!
//! `arpg-sim` must stay free of the graphics stack — not for tidiness, but
//! because it is what keeps simulation tests runnable without a GPU. Once the
//! determinism check is hashing 100k entities across two runs, having it link
//! wgpu would make the fastest test in the suite depend on a device.

use std::collections::HashSet;

const FORBIDDEN: &[(&str, &str)] = &[
    (
        "arpg-gfx",
        "the simulation describes itself in arpg_core::Instance and never learns \
         how one is drawn.",
    ),
    (
        "wgpu",
        "sim must run headless. Anything needing a GPU belongs in arpg-gfx.",
    ),
    (
        "winit",
        "sim must not know a window exists; input is translated in arpg (app).",
    ),
];

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");

    let manifest = std::fs::read_to_string("Cargo.toml").expect("read own Cargo.toml");
    let declared = declared_dependencies(&manifest);

    for (crate_name, reason) in FORBIDDEN {
        if declared.contains(*crate_name) {
            panic!(
                "\n\n  arpg-sim must not depend on {crate_name}.\n\n  {reason}\n\n  \
                 If this edge is genuinely wanted, the architecture changed and \
                 this guard should be updated deliberately — in crates/sim/build.rs \
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
