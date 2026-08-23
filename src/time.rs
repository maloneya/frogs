use std::time::Instant;

/// Frame timing.
///
/// Right now this only measures. Next chunk it grows the fixed-timestep
/// accumulator, and becomes the thing that decides how many simulation steps a
/// frame is worth — which is why it's its own module rather than two fields on
/// `App`.
pub struct Clock {
    last: Instant,
    /// Exponential moving average of frame time, in seconds.
    smoothed: f32,
    since_hud: f32,
}

/// How much each new sample moves the average. Low enough to be readable,
/// high enough to react. Note this deliberately *hides* spikes — an average is
/// the wrong instrument for pacing variance, which is what a frame-time graph
/// would show instead.
const SMOOTHING: f32 = 0.1;

/// Refreshing the title every frame is unreadable, and pushes more work at the
/// window server than the renderer is doing.
const HUD_INTERVAL: f32 = 0.1;

impl Default for Clock {
    fn default() -> Self {
        Self { last: Instant::now(), smoothed: 1.0 / 60.0, since_hud: 0.0 }
    }
}

impl Clock {
    /// Call once per frame. Returns the raw delta in seconds.
    pub fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;

        self.smoothed += (dt - self.smoothed) * SMOOTHING;
        self.since_hud += dt;
        dt
    }

    pub fn frame_ms(&self) -> f32 {
        self.smoothed * 1000.0
    }

    pub fn fps(&self) -> f32 {
        if self.smoothed > 0.0 { 1.0 / self.smoothed } else { 0.0 }
    }

    /// True roughly ten times a second.
    pub fn hud_due(&mut self) -> bool {
        if self.since_hud >= HUD_INTERVAL {
            self.since_hud = 0.0;
            return true;
        }
        false
    }
}
