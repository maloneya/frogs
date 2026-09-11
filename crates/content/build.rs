//! Guards shared content decoding with a dependency allowlist.
//!
//! Both production and tests load scenes here. Depending on the scenario
//! runner would reverse that boundary; depending on graphics would make the
//! headless gate need a GPU. Definitions belong to game and sim, not a mirror schema.

const CRATE: &str = "arpg-content";
const FILE: &str = "crates/content/build.rs";

const REASON: &str = "content decodes game- and sim-owned definitions for both app and scenario; \
                      it must not depend on the renderer, window system, or test runner.";

/// Game and sim supply definitions and validation; RON supplies file-format plumbing.
const ALLOWED: &[&str] = &["arpg-sim", "arpg-game", "ron"];

#[path = "../../build_support/dependencies.rs"]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
