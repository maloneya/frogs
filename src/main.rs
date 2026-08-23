//! A from-scratch isometric action-RPG engine, built to understand the layers
//! rather than to ship a game.
//!
//! The one structural rule: `gfx` never knows what an enemy is. Its vocabulary
//! is [`gfx::Instance`] — position, scale, colour — and `world` describes itself
//! in that vocabulary. The dependency runs one way, and `app` is the only module
//! that sees both sides.

mod app;
mod gfx;
mod time;
mod world;

fn main() {
    // RUST_LOG=info to see adapter selection and wgpu diagnostics.
    env_logger::init();
    app::App::run();
}
