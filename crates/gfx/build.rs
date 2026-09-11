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
    &["arpg-core", "bytemuck", "fontdue", "glam", "log", "png", "wgpu", "winit", "pollster"];

#[path = "../../build_support/dependencies.rs"]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
