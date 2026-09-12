//! Keeps the interchange boundary independent of simulation and presentation.
//!
//! Parsing glTF is plumbing; deciding what an asset means in a run belongs to
//! app, and turning it into GPU resources belongs to gfx.

const CRATE: &str = "arpg-assets";
const FILE: &str = "crates/assets/build.rs";

const REASON: &str = "assets validates interchange data into an engine-owned CPU representation; \
                      it must not know simulation, gameplay, windows, or GPU resources.";

/// Binary layout, maths, and the interchange parser are the complete authority.
const ALLOWED: &[&str] = &["bytemuck", "glam", "gltf", "png"];

#[path = "../../build_support/dependencies.rs"]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
