//! Authoring is headless and cannot depend on the application or renderer.
const CRATE: &str = "arpg-authoring";
const FILE: &str = "crates/authoring/build.rs";
const REASON: &str = "authoring runs independently of the game process, window system and GPU";
const ALLOWED: &[&str] = &["arpg-core", "arpg-sim", "arpg-game", "arpg-content", "glam", "serde", "serde_json"];
#[path = "../../build_support/dependencies.rs"]
mod dependencies;
fn main() { dependencies::enforce(CRATE, ALLOWED, REASON, FILE); }
