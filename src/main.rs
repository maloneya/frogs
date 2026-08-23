mod app;
mod gfx;

fn main() {
    // RUST_LOG=info to see adapter selection and wgpu diagnostics.
    env_logger::init();
    app::App::run();
}
