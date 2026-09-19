//! Bounded, presentation-only performance observations. Never hashed or read by gameplay.
//!
//! A sample runs from one redraw start to the next. Its work belongs to the
//! first redraw, so a slow tick and its long frame remain on the same row.
//! Collection allocates once; sorting and JSON construction happen only on request.

use std::time::{Duration, Instant};

use arpg_core::Report;

const CAPACITY: usize = 1024;
/// Diagnostic target, independent of the simulation tick rate and display refresh rate.
const TARGET_HZ: u32 = 60;
const FRAME_BUDGET: Duration = Duration::from_nanos(1_000_000_000 / TARGET_HZ as u64);

/// Wall-clock timing stays in app; sim provides only phase boundaries and counts.
#[derive(Default)]
pub(crate) struct CrowdTimer {
    started: Option<Instant>,
    pub(crate) elapsed: Duration,
    pub(crate) candidates: u64,
    pub(crate) possible_pairs: u64,
}

impl arpg_sim::StepObserver for CrowdTimer {
    fn crowd_started(&mut self) { self.started = Some(Instant::now()); }
    fn crowd_finished(&mut self, work: arpg_sim::CrowdWork) {
        self.elapsed += self.started.take().expect("crowd phase started").elapsed();
        self.candidates += work.candidates();
        self.possible_pairs += work.possible_pairs();
    }
}

/// Work measured by the frame loop. Durations cannot contain NaN or negative values.
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameWork {
    pub(crate) simulation: Duration,
    /// Subset of simulation: only calls to Game::step, summed across this frame.
    pub(crate) game_step: Duration,
    /// Subset of Game::step, summed over this frame's ticks.
    pub(crate) crowd: Duration,
    pub(crate) crowd_candidates: u64,
    pub(crate) crowd_possible_pairs: u64,
    pub(crate) presentation: Duration,
    /// Includes surface acquisition, uploads, submission and present; not GPU execution time.
    pub(crate) render_cpu: Duration,
    pub(crate) ticks: u32,
    pub(crate) end_tick: u64,
    pub(crate) enemies: usize,
    /// Last completed tick's resolved contacts, not a sum across this frame.
    pub(crate) crowd_contacts: usize,
    pub(crate) presented: bool,
    pub(crate) focused: bool,
    pub(crate) occluded: bool,
}

#[derive(Clone, Copy)]
struct Sample {
    interval: Duration,
    work: FrameWork,
}

pub(crate) struct Recorder {
    samples: Vec<Sample>,
    next: usize,
    overwritten: u64,
    started: Option<Instant>,
    pending: Option<FrameWork>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            samples: Vec::with_capacity(CAPACITY), next: 0, overwritten: 0,
            started: None, pending: None,
        }
    }
}

impl Recorder {
    /// Complete the previous frame before starting this one. The first start
    /// after reset establishes a baseline, excluding loading or the previous run.
    pub(crate) fn begin_frame(&mut self, now: Instant) {
        if let (Some(start), Some(work)) = (self.started, self.pending.take()) {
            let sample = Sample { interval: now.duration_since(start), work };
            if self.samples.len() < CAPACITY {
                self.samples.push(sample);
            } else {
                self.samples[self.next] = sample;
                self.overwritten = self.overwritten.saturating_add(1);
            }
            self.next = (self.next + 1) % CAPACITY;
        }
        self.started = Some(now);
    }

    pub(crate) fn finish_frame(&mut self, work: FrameWork) {
        assert!(self.started.is_some(), "a frame must start before it finishes");
        assert!(self.pending.is_none(), "a frame finishes only once");
        self.pending = Some(work);
    }

    /// Retain allocated storage, but discard both old samples and the unfinished interval.
    pub(crate) fn reset(&mut self) {
        self.samples.clear();
        self.next = 0;
        self.overwritten = 0;
        self.started = None;
        self.pending = None;
    }

    fn ordered(&self) -> impl Iterator<Item = &Sample> {
        let first = if self.samples.len() == CAPACITY { self.next } else { 0 };
        self.samples[first..].iter().chain(self.samples[..first].iter())
    }

    /// Reports only the bounded window; no world traversal, hashing or mutation.
    pub(crate) fn report(&self, out: &mut Report, raw: bool) {
        out.int("capacity", CAPACITY as u64);
        out.int("samples", self.samples.len() as u64);
        out.int("overwritten", self.overwritten);
        out.int("target_hz", u64::from(TARGET_HZ));
        out.num("budget_ms", millis(FRAME_BUDGET));
        out.bool("gpu_timing_available", false);
        let total: Duration = self.samples.iter().map(|s| s.interval).sum();
        let ticks: u64 = self.samples.iter().map(|s| u64::from(s.work.ticks)).sum();
        let presented = self.samples.iter().filter(|s| s.work.presented).count();
        out.num("window_ms", millis(total));
        out.int("presented", presented as u64);
        out.int("surface_skipped", (self.samples.len() - presented) as u64);
        out.int("unfocused_frames", self.samples.iter().filter(|s| !s.work.focused).count() as u64);
        out.int("occluded_frames", self.samples.iter().filter(|s| s.work.occluded).count() as u64);
        out.num("presented_fps", rate(presented as u64, total));
        out.int("ticks", ticks);
        out.num("ticks_per_second", rate(ticks, total));
        out.int("max_ticks_per_frame", self.samples.iter().map(|s| u64::from(s.work.ticks)).max().unwrap_or(0));
        out.int("zero_tick_frames", self.samples.iter().filter(|s| s.work.ticks == 0).count() as u64);
        out.int("multi_tick_frames", self.samples.iter().filter(|s| s.work.ticks > 1).count() as u64);
        // Crossing the target by a few microseconds can be vsync jitter; neither
        // threshold claims to count missed physical display presentations.
        out.int("over_budget", self.samples.iter().filter(|s| s.interval > FRAME_BUDGET).count() as u64);
        out.int("over_1_5_budget", self.samples.iter().filter(|s| s.interval > FRAME_BUDGET.mul_f64(1.5)).count() as u64);
        let stepping: Duration = self.samples.iter().map(|s| s.work.game_step).sum();
        out.num("mean_step_ms", if ticks == 0 { 0.0 } else { millis(stepping) / ticks as f32 });
        let crowd: Duration = self.samples.iter().map(|s| s.work.crowd).sum();
        out.num("mean_crowd_step_ms", if ticks == 0 { 0.0 } else { millis(crowd) / ticks as f32 });
        out.int("crowd_candidates", self.samples.iter().map(|s| s.work.crowd_candidates).sum());
        out.int("crowd_possible_pairs", self.samples.iter().map(|s| s.work.crowd_possible_pairs).sum());
        for (name, get) in METRICS {
            let values = self.samples.iter().map(get).collect();
            out.object(name, |out| distribution(out, values));
        }
        if raw {
            out.object("frames", |out| {
                for (i, sample) in self.ordered().enumerate() {
                    out.object(&i.to_string(), |out| {
                        for (name, get) in METRICS { out.num(name, millis(get(sample))); }
                        let work = sample.work;
                        out.int("ticks", u64::from(work.ticks));
                        out.int("end_tick", work.end_tick);
                        out.int("enemies", work.enemies as u64);
                        out.int("crowd_contacts", work.crowd_contacts as u64);
                        out.int("crowd_candidates", work.crowd_candidates);
                        out.int("crowd_possible_pairs", work.crowd_possible_pairs);
                        out.bool("presented", work.presented);
                        out.bool("focused", work.focused);
                        out.bool("occluded", work.occluded);
                    });
                }
            });
        }
    }
}

/// Shared vocabulary for summaries and raw observations. `game_step` is nested
/// inside simulation; other_ms covers HUD title work, event processing and waiting.
type Metric = (&'static str, fn(&Sample) -> Duration);
const METRICS: [Metric; 7] = [
    ("frame_ms", |s| s.interval),
    ("simulation_ms", |s| s.work.simulation),
    ("game_step_ms", |s| s.work.game_step),
    ("crowd_ms", |s| s.work.crowd),
    ("presentation_ms", |s| s.work.presentation),
    ("render_cpu_ms", |s| s.work.render_cpu),
    ("other_ms", |s| s.interval.saturating_sub(s.work.simulation + s.work.presentation + s.work.render_cpu)),
];

fn millis(value: Duration) -> f32 { value.as_secs_f32() * 1000.0 }

fn rate(count: u64, elapsed: Duration) -> f32 {
    if elapsed.is_zero() { 0.0 } else { (count as f64 / elapsed.as_secs_f64()) as f32 }
}

/// Nearest-rank percentiles, computed only when requested; empty windows report zeros.
fn distribution(out: &mut Report, mut values: Vec<Duration>) {
    values.sort_unstable();
    let percentile = |percent: usize| {
        values.get((values.len() * percent).div_ceil(100).saturating_sub(1)).copied().unwrap_or_default()
    };
    out.num("mean", if values.is_empty() { 0.0 } else { millis(values.iter().copied().sum()) / values.len() as f32 });
    out.num("p50", millis(percentile(50)));
    out.num("p95", millis(percentile(95)));
    out.num("p99", millis(percentile(99)));
    out.num("max", millis(values.last().copied().unwrap_or_default()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(recorder: &Recorder, raw: bool) -> String {
        let mut out = Report::default();
        recorder.report(&mut out, raw);
        out.finish()
    }

    #[test]
    fn a_stall_stays_with_its_work_and_catch_up_counts() {
        let mut recorder = Recorder::default();
        let start = Instant::now();
        recorder.begin_frame(start);
        recorder.finish_frame(FrameWork {
            simulation: Duration::from_millis(30), game_step: Duration::from_millis(28),
            presentation: Duration::from_millis(2), render_cpu: Duration::from_millis(3),
            ticks: 2, end_tick: 2, presented: true, ..FrameWork::default()
        });
        assert!(report(&recorder, true).contains("\"samples\":0"), "not yet a complete interval");
        recorder.begin_frame(start + Duration::from_millis(40));
        recorder.finish_frame(FrameWork { end_tick: 2, ..FrameWork::default() });
        recorder.begin_frame(start + Duration::from_millis(50));
        let json = report(&recorder, true);
        for expected in ["\"presented_fps\":20.0000", "\"ticks_per_second\":40.0000", "\"mean_step_ms\":14.0000", "\"zero_tick_frames\":1", "\"multi_tick_frames\":1", "\"surface_skipped\":1", "\"over_budget\":1", "\"other_ms\":5.0000", "\"0\":{\"frame_ms\":40.0000,\"simulation_ms\":30.0000", "\"1\":{\"frame_ms\":10.0000,\"simulation_ms\":0.0000"] {
            assert!(json.contains(expected), "missing {expected}: {json}");
        }
    }

    #[test]
    fn retention_is_bounded_ordered_and_reset_drops_pending_work() {
        let mut recorder = Recorder::default();
        let start = Instant::now();
        for i in 0..CAPACITY + 4 {
            recorder.begin_frame(start + Duration::from_millis(i as u64));
            recorder.finish_frame(FrameWork { end_tick: i as u64, ..FrameWork::default() });
        }
        assert_eq!(recorder.samples.len(), CAPACITY);
        assert_eq!(recorder.overwritten, 3);
        assert_eq!(recorder.ordered().next().unwrap().work.end_tick, 3);
        assert_eq!(recorder.ordered().last().unwrap().work.end_tick, (CAPACITY + 2) as u64);
        let capacity = recorder.samples.capacity();
        recorder.reset();
        recorder.begin_frame(start + Duration::from_secs(60));
        assert!(recorder.samples.is_empty());
        assert_eq!(recorder.samples.capacity(), capacity);
        assert_eq!(recorder.overwritten, 0);
        assert!(report(&recorder, true).contains("\"presented_fps\":0.0000"));
    }

    #[test]
    fn percentiles_preserve_spikes_hidden_by_the_mean() {
        let mut out = Report::default();
        distribution(&mut out, (1..=100).rev().map(Duration::from_millis).collect());
        assert_eq!(out.finish(), r#"{"mean":50.5000,"p50":50.0000,"p95":95.0000,"p99":99.0000,"max":100.0000}"#);
    }

    #[test]
    fn invalid_presentation_conditions_follow_the_completed_interval() {
        let mut recorder = Recorder::default();
        let start = Instant::now();
        recorder.begin_frame(start);
        recorder.finish_frame(FrameWork { focused: false, occluded: true, ..Default::default() });
        assert!(report(&recorder, false).contains("\"unfocused_frames\":0"));
        recorder.begin_frame(start + Duration::from_millis(16));
        let json = report(&recorder, true);
        for field in ["\"unfocused_frames\":1", "\"occluded_frames\":1", "\"focused\":false", "\"occluded\":true"] {
            assert!(json.contains(field), "{field}");
        }
        recorder.reset();
        assert!(report(&recorder, false).contains("\"occluded_frames\":0"));
    }

    #[test]
    fn crowd_totals_share_the_frame_window_and_reset_with_it() {
        let mut recorder = Recorder::default();
        let start = Instant::now();
        recorder.begin_frame(start);
        recorder.finish_frame(FrameWork {
            ticks: 2, simulation: Duration::from_millis(8), game_step: Duration::from_millis(7),
            crowd: Duration::from_millis(6), crowd_candidates: 30,
            crowd_possible_pairs: 1000, ..Default::default()
        });
        recorder.begin_frame(start + Duration::from_millis(16));
        recorder.finish_frame(FrameWork::default());
        recorder.begin_frame(start + Duration::from_millis(20));
        let json = report(&recorder, true);
        for field in ["\"mean_crowd_step_ms\":3.0000", "\"crowd_candidates\":30", "\"crowd_possible_pairs\":1000", "\"crowd_ms\":6.0000"] {
            assert!(json.contains(field), "{field}");
        }
        recorder.reset();
        assert!(report(&recorder, false).contains("\"crowd_candidates\":0"));
    }
}
