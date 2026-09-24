//! Unpaced release trials through Game; clocks only surround the step call.
use std::{io::Write, path::Path, process::ExitCode, time::Instant};
use arpg_core::Report;
use arpg_game::Game;
use arpg_sim::{Accumulator, Dt};

#[path = "../../../../benchmarks/driver.rs"]
mod driver;

fn run() -> Result<String, String> {
    if cfg!(debug_assertions) { return Err("headless performance requires --release".into()); }
    let mut args = std::env::args().skip(1);
    let scene = args.next().ok_or("expected scene warmup samples idle|combat")?;
    let spec = driver::Spec::parse(args.collect::<Vec<_>>().join(" ").split_whitespace())?;
    let scene = arpg_content::load_scene(Path::new(&scene)).map_err(|e| e.to_string())?;
    let mut game = Game::from_scene(&scene).map_err(|e| e.to_string())?;
    let initial = game.enemy_count();
    let dt = Accumulator::default().pending(Dt::SECS).next().expect("one fixed tick");
    for tick in 0..spec.warmup { game.step(dt, spec.intent(tick)); }
    let start = game.enemy_count();
    let mut population = 0;
    let mut timings = Vec::with_capacity(spec.samples as usize);
    for tick in spec.warmup..spec.warmup + spec.samples {
        let intent = spec.intent(tick);
        let began = Instant::now();
        game.step(dt, intent);
        timings.push(began.elapsed().as_nanos() as u64);
        population += game.enemy_count() as u64;
    }
    let mut out = Report::default();
    out.int("schedule_version", 1);
    out.int("warmup_ticks", spec.warmup);
    out.int("sample_ticks", spec.samples);
    out.text("input", if spec.combat { "combat" } else { "idle" });
    out.bool("debug_build", cfg!(debug_assertions));
    out.int("initial_enemies", initial as u64);
    out.int("start_enemies", start as u64);
    out.int("end_enemies", game.enemy_count() as u64);
    out.int("population_sum", population);
    out.int("current_tick", game.tick());
    out.bool("finite", game.all_positions_finite());
    out.text("final_hash", &format!("{:016x}", game.hash()));
    out.object("step_ns", |out| {
        for (index, nanos) in timings.iter().enumerate() { out.int(&index.to_string(), *nanos); }
    });
    Ok(out.finish())
}

fn main() -> ExitCode {
    match run() {
        Ok(json) => { let _ = writeln!(std::io::stdout(), "{json}"); ExitCode::SUCCESS }
        Err(error) => { let _ = writeln!(std::io::stderr(), "{error}"); ExitCode::FAILURE }
    }
}
