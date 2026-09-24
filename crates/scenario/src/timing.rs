//! Coarse runner costs, separate from simulation state and its measured step budget.
use std::time::Duration;
use arpg_core::Report;

#[derive(Default)]
pub(crate) struct Timing {
    pub(crate) setup: Duration,
    pub(crate) simulation: Duration,
    pub(crate) hashing: Duration,
    pub(crate) assertions: Duration,
    pub(crate) driver: Duration,
}

#[derive(Default)]
pub(crate) struct Sample {
    pub(crate) file: String,
    pub(crate) passed: bool,
    pub(crate) first: Timing,
    pub(crate) replay: Duration,
    pub(crate) total: Duration,
}

impl Sample {
    pub(crate) fn report(&self, out: &mut Report) {
        out.text("file", &self.file);
        out.bool("passed", self.passed);
        out.num("total_ms", self.total.as_secs_f32() * 1000.0);
        out.num("setup_ms", self.first.setup.as_secs_f32() * 1000.0);
        out.num("first_simulation_ms", self.first.simulation.as_secs_f32() * 1000.0);
        out.num("first_hashing_ms", self.first.hashing.as_secs_f32() * 1000.0);
        out.num("first_assertions_ms", self.first.assertions.as_secs_f32() * 1000.0);
        out.num("first_driver_ms", self.first.driver.as_secs_f32() * 1000.0);
        out.num("replay_ms", self.replay.as_secs_f32() * 1000.0);
    }
}

pub(crate) fn write(path: &std::path::Path, samples: &[Sample]) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = Report::default();
    out.int("schema", 1);
    out.object("scenarios", |out| {
        for (index, sample) in samples.iter().enumerate() {
            out.object(&index.to_string(), |out| sample.report(out));
        }
    });
    // Exclusive creation: historical timing evidence cannot be overwritten by accident.
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(out.finish().as_bytes())
}
