use std::time::Instant;

/// Frame timing.
///
/// This measures, and only measures. Deciding how much *simulation* a frame is
/// worth belongs to `sim::Accumulator`, next to the `Dt` it mints — so the one
/// crate that can read a clock cannot convert what it reads into simulation
/// time, and the one crate that defines simulation time cannot read a clock.
pub(crate) struct Clock {
    last: Instant,
    /// Exponential moving average of frame time, in seconds.
    smoothed: f32,
}

/// How much each new sample moves the average. Low enough to be readable,
/// high enough to react. Note this deliberately *hides* spikes — an average is
/// the wrong instrument for pacing variance, which is what a frame-time graph
/// would show instead.
const SMOOTHING: f32 = 0.1;
const _: () = assert!(SMOOTHING > 0.0 && SMOOTHING <= 1.0, "outside (0, 1] the average diverges or freezes");

impl Default for Clock {
    fn default() -> Self {
        Self { last: Instant::now(), smoothed: 1.0 / 60.0 }
    }
}

impl Clock {
    /// Unclamped elapsed seconds. The simulation accumulator caps catch-up;
    /// telemetry must retain the actual hitch duration.
    pub(crate) fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;

        self.smoothed += (dt - self.smoothed) * SMOOTHING;
        dt
    }

    pub(crate) fn frame_ms(&self) -> f32 {
        self.smoothed * 1000.0
    }

    pub(crate) fn fps(&self) -> f32 {
        if self.smoothed > 0.0 { 1.0 / self.smoothed } else { 0.0 }
    }

}
