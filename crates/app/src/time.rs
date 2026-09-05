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
    since_hud: f32,
}

/// How much each new sample moves the average. Low enough to be readable,
/// high enough to react. Note this deliberately *hides* spikes — an average is
/// the wrong instrument for pacing variance, which is what a frame-time graph
/// would show instead.
const SMOOTHING: f32 = 0.1;
const _: () = assert!(SMOOTHING > 0.0 && SMOOTHING <= 1.0, "outside (0, 1] the average diverges or freezes");

/// Refreshing the title every frame is unreadable, and pushes more work at the
/// window server than the renderer is doing.
const HUD_INTERVAL: f32 = 0.1;
const _: () = assert!(HUD_INTERVAL > 0.0);

impl Default for Clock {
    fn default() -> Self {
        Self { last: Instant::now(), smoothed: 1.0 / 60.0, since_hud: 0.0 }
    }
}

impl Clock {
    /// Call once per frame. Returns how long the frame took, in seconds.
    ///
    /// Unclamped, and that is a change: this used to cap the value at 0.1s so a
    /// stalled frame could not teleport the player through a wall. The cap did
    /// not disappear, it moved — `sim`'s accumulator now caps the number of
    /// *ticks* a frame may run, which is the same guard expressed in the unit
    /// that decides it, and enforced by the only code that can mint a step.
    ///
    /// So what leaves here is the honest measurement, which is what the HUD and
    /// the harness want anyway: a clamp reaching into the frame-time readout
    /// would hide the very hitches it exists to report.
    pub(crate) fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;

        self.smoothed += (dt - self.smoothed) * SMOOTHING;
        self.since_hud += dt;
        dt
    }

    pub(crate) fn frame_ms(&self) -> f32 {
        self.smoothed * 1000.0
    }

    pub(crate) fn fps(&self) -> f32 {
        if self.smoothed > 0.0 { 1.0 / self.smoothed } else { 0.0 }
    }

    /// True roughly ten times a second.
    pub(crate) fn hud_due(&mut self) -> bool {
        if self.since_hud >= HUD_INTERVAL {
            self.since_hud = 0.0;
            return true;
        }
        false
    }
}
