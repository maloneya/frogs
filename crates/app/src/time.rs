use std::time::Instant;

/// Frame timing.
///
/// Right now this only measures. Next chunk it grows the fixed-timestep
/// accumulator, and becomes the thing that decides how many simulation steps a
/// frame is worth — which is why it's its own module rather than two fields on
/// `App`.
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

/// The longest delta the simulation is allowed to see, in seconds.
///
/// Dragging the window, waiting on a shader compile or sitting at a breakpoint
/// produces a frame worth hundreds of milliseconds. Integrated honestly that is
/// a teleport — through a wall, past a hitbox, out of the arena. Every game
/// clamps this somewhere; the choice is only whether it happens on purpose.
///
/// ~6 frames at 60Hz. Beyond that the game deliberately runs in slow motion
/// rather than skipping space, which is the right trade when the alternative is
/// losing collisions.
const MAX_FRAME_TIME: f32 = 0.1;
const _: () = assert!(
    MAX_FRAME_TIME >= 1.0 / 60.0,
    "clamping below a real frame would run the game permanently in slow motion"
);

impl Default for Clock {
    fn default() -> Self {
        Self { last: Instant::now(), smoothed: 1.0 / 60.0, since_hud: 0.0 }
    }
}

impl Clock {
    /// Call once per frame. Returns the delta to simulate, in seconds, clamped
    /// to [`MAX_FRAME_TIME`].
    ///
    /// The measurement side is fed the *raw* value on purpose: the clamp exists
    /// to protect the simulation, and letting it reach into the HUD as well
    /// would hide the very hitches the HUD is there to show.
    pub(crate) fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;

        self.smoothed += (dt - self.smoothed) * SMOOTHING;
        self.since_hud += dt;
        dt.min(MAX_FRAME_TIME)
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
