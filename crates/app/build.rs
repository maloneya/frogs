//! The runtime application must not host the authoring backend.
const CRATE: &str = "arpg";
const FILE: &str = "crates/app/build.rs";
const REASON: &str = "the app consumes playable content and runtime controls, never the authoring backend";
const ALLOWED: &[&str] = &["arpg-assets", "arpg-core", "arpg-gfx", "arpg-sim", "arpg-game", "arpg-content",
    "env_logger", "glam", "log", "kira", "pollster", "winit", "serde", "serde_json", "toml_edit"];
#[path = "../../build_support/dependencies.rs"]
mod dependencies;
fn main() { dependencies::enforce(CRATE, ALLOWED, REASON, FILE); }
