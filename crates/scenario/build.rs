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

/// The complete set. `arpg-game` is what is under test; `arpg-sim`,
/// `arpg-core` and `glam` supply the shared vocabulary. `arpg-content` loads shared scenes; `ron`/`serde` parse
/// assertion scripts, which belong only to this runner.
const ALLOWED: &[&str] = &["arpg-core", "arpg-sim", "arpg-game", "arpg-content", "glam", "ron", "serde"];

#[path = "../../build_support/dependencies.rs"]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
