//! Fixed ticks from elapsed frame time, with a bounded catch-up budget.

/// Simulation ticks per second; combat timing uses the 60 Hz frame convention.
pub const TICK_HZ: u32 = 60;
const _: () = assert!(TICK_HZ > 0, "a zero tick rate makes Dt::SECS a division by zero");

/// Maximum catch-up ticks per frame. Excess time is discarded to prevent an
/// accumulating backlog; overload slows game time rather than skipping motion.
const MAX_TICKS_PER_FRAME: u32 = 6;
const _: () = assert!(MAX_TICKS_PER_FRAME >= 1, "a frame that can run no ticks freezes the game");

/// A fixed-duration token minted only by [`Accumulator::pending`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dt(());

impl Dt {
    /// The fixed step, in seconds.
    pub const SECS: f32 = 1.0 / TICK_HZ as f32;

    /// The step, in seconds — always [`Dt::SECS`].
    #[inline]
    #[must_use]
    pub fn secs(self) -> f32 {
        Self::SECS
    }
}

/// Converts elapsed seconds into fixed ticks and retains the fractional remainder.
#[derive(Default)]
pub struct Accumulator {
    /// Time owed but not yet worth a whole tick. Always in `[0, Dt::SECS)`
    /// after [`pending`](Accumulator::pending) returns.
    carry: f32,
}

impl Accumulator {
    /// Absorbs elapsed seconds and yields fixed ticks, discarding excess catch-up time.
    pub fn pending(&mut self, frame_secs: f32) -> Ticks {
        // A monotonic clock cannot go backwards, but a harness, a test or a
        // deserialised replay can hand this anything. `max` also swallows NaN,
        // which would otherwise poison the accumulator permanently.
        self.carry += frame_secs.max(0.0);

        let owed = (self.carry / Dt::SECS).floor();

        // Every whole tick owed leaves the accumulator here, whether or not it
        // is run below. See `MAX_TICKS_PER_FRAME` for why keeping the excess
        // would be worse than dropping it.
        self.carry = (self.carry - owed * Dt::SECS).max(0.0);

        // Saturating float-to-int: a frame of a thousand seconds becomes
        // `u32::MAX`, then the cap, rather than wrapping to a small number.
        Ticks { remaining: (owed as u32).min(MAX_TICKS_PER_FRAME) }
    }

    /// Presentation interpolation fraction; simulation passes must not consume it.
    #[must_use]
    pub fn alpha(&self) -> Alpha {
        // Always in range already — `pending` removes every whole tick — so the
        // clamp is for a caller who built an accumulator and never stepped it.
        Alpha((self.carry / Dt::SECS).clamp(0.0, 1.0))
    }
}

/// Presentation fraction in 0..=1 for interpolating between the last two ticks.
/// Interpolation adds one tick of latency. Read-only World presentation methods
/// prevent writeback; the type alone cannot prevent use in simulation math.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Alpha(f32);

impl Alpha {
    /// Exactly the previous tick.
    pub const ZERO: Self = Self(0.0);
    /// Exactly the current tick — the simulation as it actually is.
    ///
    /// A frame never asks for this (the accumulator's carry is always less than
    /// a whole tick), but a test, and later a screenshot taken at a tick
    /// boundary, wants to see the state itself rather than a blend.
    pub const ONE: Self = Self(1.0);

    /// The blend factor, in `0..=1`.
    #[inline]
    #[must_use]
    pub fn get(self) -> f32 {
        self.0
    }
}

/// The ticks one frame bought. See [`Accumulator::pending`].
///
/// Yields `Dt` and nothing else, so the loop body cannot get at the frame's
/// real duration even by accident.
pub struct Ticks {
    remaining: u32,
}

impl Iterator for Ticks {
    type Item = Dt;

    fn next(&mut self) -> Option<Dt> {
        self.remaining = self.remaining.checked_sub(1)?;
        Some(Dt(()))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Ticks {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the carry: time that did not add up to a tick is not
    /// lost, it waits.
    #[test]
    fn a_frame_yields_whole_ticks_and_carries_the_remainder() {
        let mut acc = Accumulator::default();

        // Two-thirds of a tick buys nothing yet.
        assert_eq!(acc.pending(Dt::SECS * 2.0 / 3.0).len(), 0);
        // Another two-thirds makes one whole tick, with a third left over.
        assert_eq!(acc.pending(Dt::SECS * 2.0 / 3.0).len(), 1);

        let left = acc.alpha().get();
        assert!(left > 0.0 && left < 1.0, "carry must stay a fraction of a tick, got {left}");
    }

    /// A long stall must not be repaid. See `MAX_TICKS_PER_FRAME`.
    #[test]
    fn a_stall_cannot_spiral() {
        let mut acc = Accumulator::default();

        assert_eq!(acc.pending(10.0).len() as u32, MAX_TICKS_PER_FRAME);
        assert!(
            acc.alpha().get() < 1.0,
            "the debt from a stalled frame was kept, so the next frame owes even more"
        );
        // The frame after a stall is an ordinary frame again.
        assert_eq!(acc.pending(Dt::SECS).len(), 1);
    }

    /// Garbage in must not corrupt the accumulator, because it would stay
    /// corrupt for the rest of the run.
    #[test]
    fn a_nonsense_frame_delta_is_absorbed() {
        let mut acc = Accumulator::default();

        assert_eq!(acc.pending(f32::NAN).len(), 0);
        assert_eq!(acc.pending(-1.0).len(), 0);
        assert!(acc.alpha().get().is_finite());

        assert_eq!(acc.pending(Dt::SECS).len(), 1, "the accumulator stopped working after bad input");
    }

    #[test]
    fn alpha_tracks_the_way_through_a_tick() {
        let mut acc = Accumulator::default();

        assert_eq!(acc.alpha(), Alpha::ZERO, "a fresh accumulator is on a tick boundary");

        acc.pending(Dt::SECS / 4.0);
        assert!((acc.alpha().get() - 0.25).abs() < 1e-6, "got {}", acc.alpha().get());

        acc.pending(Dt::SECS / 4.0);
        assert!((acc.alpha().get() - 0.5).abs() < 1e-6, "got {}", acc.alpha().get());

        // Crossing a tick boundary resets the blend rather than running past it.
        acc.pending(Dt::SECS / 2.0);
        assert_eq!(acc.alpha(), Alpha::ZERO);
    }

    #[test]
    fn alpha_never_leaves_its_range() {
        let mut acc = Accumulator::default();
        for frame in [0.0, f32::NAN, -5.0, 1000.0, Dt::SECS * 0.999, Dt::SECS * 5.5] {
            acc.pending(frame);
            let a = acc.alpha().get();
            assert!((0.0..=1.0).contains(&a), "alpha {a} out of range after a {frame}s frame");
        }
    }

    /// Exactness matters more here than it looks: it is what lets a scenario
    /// say "at tick 417" and mean it.
    #[test]
    fn a_run_of_whole_ticks_does_not_drift() {
        let mut acc = Accumulator::default();
        let mut ticks = 0;

        for _ in 0..600 {
            ticks += acc.pending(Dt::SECS).len();
        }

        assert_eq!(ticks, 600, "feeding exactly one tick per frame produced a different count");
        assert_eq!(acc.alpha(), Alpha::ZERO, "whole ticks left a remainder behind");
    }
}
