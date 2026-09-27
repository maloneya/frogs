//! Dependency allowlist for the handwritten, device-independent simulation.

const CRATE: &str = "arpg-sim";
const FILE: &str = "crates/sim/build.rs";

const REASON: &str = "sim must stay free of the graphics stack and of the window system, \
                      because that is what keeps simulation tests runnable without a GPU; \
                      and the engine layers are hand-written on purpose, so an engine or \
                      an off-the-shelf ECS defeats the exercise.";

/// Shared vocabulary, maths and data decoding; file I/O stays in adapters.
const ALLOWED: &[&str] = &["arpg-core", "glam", "serde"];

#[path = "../../build_support/dependencies.rs"]
#[expect(clippy::disallowed_methods, reason = "the build-time dependency guard reads manifests; simulation code cannot read files")]
mod dependencies;

fn main() {
    dependencies::enforce(CRATE, ALLOWED, REASON, FILE);
}
