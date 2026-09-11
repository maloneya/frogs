//! Guards the simulation's dependency graph, as an **allowlist**.
//!
//! A denylist was here first and it failed open: it named `arpg-gfx`, `wgpu` and
//! `winit`, so it caught exactly the three mistakes someone had already thought
//! of. Adding `bevy`, `hecs` or `rapier` to this crate sailed straight through —
//! and "do not introduce a game engine or an off-the-shelf ECS" is the loudest
//! rule the project has, because writing those layers is the entire point.
//!
//! An allowlist fails closed. Anything not named below stops the build,
//! including the dependency nobody predicted.

const CRATE: &str = "arpg-sim";
const FILE: &str = "crates/sim/build.rs";

const REASON: &str = "sim must stay free of the graphics stack and of the window system, \
                      because that is what keeps simulation tests runnable without a GPU; \
                      and the engine layers are hand-written on purpose, so an engine or \
                      an off-the-shelf ECS defeats the exercise.";

/// The complete set. `arpg-core` for shared vocabulary, `glam` for maths,
/// `serde` for the written form of the game vocabulary.
///
/// `serde` is plumbing rather than one of the layers being studied, and it is
/// what lets the scenario format and the harness derive from the simulation's
/// own types rather than restating them. It links no I/O and no format: `ron`
/// stays in `content` and `scenario`, where the files are.
const ALLOWED: &[&str] = &["arpg-core", "glam", "serde"];

#[path = "../../build_support/dependencies.rs"]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
