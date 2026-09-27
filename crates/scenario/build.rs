//! Dependency allowlist keeping scenarios independent of graphics and devices.

const CRATE: &str = "scenario";
const FILE: &str = "crates/scenario/build.rs";

const REASON: &str = "a scenario must run headless, with no GPU, no window and no wall clock, \
                      because that is what makes it cheap enough to be the gate on every \
                      change; and it asserts what the simulation does, so it has no business \
                      naming a renderer or a window system.";

/// The complete set. `arpg-game` is what is under test; `arpg-sim`,
/// `arpg-core` and `glam` supply the shared vocabulary. `ron`/`serde` parse
/// assertion scripts, which belong only to this runner. `arpg-content` loads
/// benchmark-owned scene documents for the separate perf-headless binary.
const ALLOWED: &[&str] = &["arpg-core", "arpg-sim", "arpg-game", "arpg-content", "glam", "ron", "serde"];

#[path = "../../build_support/dependencies.rs"]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
