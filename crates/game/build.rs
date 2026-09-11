//! Keeps gameplay independent of file loading, rendering, and test adapters.
//!
//! App and scenario drive the same game. Gameplay consumes typed descriptions
//! and fixed ticks; adapters own files, clocks, devices, and assertions.

const CRATE: &str = "arpg-game";
const FILE: &str = "crates/game/build.rs";

const REASON: &str = "game must remain deterministic and headless; it may depend on sim \
                      and shared vocabulary, not content I/O, graphics, or the scenario runner.";

/// Engine operations, shared inputs/output, and mathematical value types.
const ALLOWED: &[&str] = &["arpg-core", "arpg-sim", "glam", "serde"];

#[path = "../../build_support/dependencies.rs"]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
