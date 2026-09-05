//! The tick — the unit simulation time is measured in, and the only door it
//! can be minted through.
//!
//! Everything the project wants to build next is indexed by ticks rather than
//! by seconds: trace events are tick-stamped, scenario inputs are scheduled at
//! a tick, attack windows are counted in ticks, and a replay compares two runs
//! tick by tick. None of that has a unit to count in while `step` integrates
//! whatever wall-clock delta the frame happened to take.
//!
//! So the frame rate stops reaching the simulation here. A frame hands its
//! elapsed seconds to an [`Accumulator`], which hands back whole [`Dt`]s and
//! keeps the remainder for next time.

/// Simulation ticks per second.
///
/// 60 rather than something higher for two reasons. It matches the display, so
/// the interpolation factor stays near-constant and pacing artefacts stay
/// visible instead of being averaged away. And combat here will be specified in
/// frames — "startup 8, active 4, recovery 12", "hitstop 4" — which is the
/// vocabulary of 60Hz games; at 120 every one of those numbers doubles and
/// stops matching the reference material it was chosen against.
pub const TICK_HZ: u32 = 60;
const _: () = assert!(TICK_HZ > 0, "a zero tick rate makes Dt::SECS a division by zero");

/// The longest a single frame may push the simulation, in ticks.
///
/// Inherited from the `MAX_FRAME_TIME` clamp this replaces, and it is the same
/// 0.1s expressed in the unit that now matters. Dragging the window, compiling
/// a shader or sitting at a breakpoint produces a frame worth hundreds of
/// milliseconds; run honestly that is a teleport, through a wall and past a
/// hitbox.
///
/// The cap and the discard in [`Accumulator::pending`] have to be read
/// together. Capping the steps while *keeping* the time owed is the spiral of
/// death: a machine that falls behind owes more every frame and never catches
/// up. Discarding it means the game runs in slow motion for a moment rather
/// than skipping space — which is the right trade when the alternative is
/// losing collisions.
const MAX_TICKS_PER_FRAME: u32 = 6;
const _: () = assert!(MAX_TICKS_PER_FRAME >= 1, "a frame that can run no ticks freezes the game");

/// One tick of simulation time.
///
/// **It deliberately carries no number.** The obvious design is a newtype
/// around `f32` with a private constructor, which stops a caller *building* a
/// variable timestep but still lets one be carried around once minted. A unit
/// struct makes the wrong value unrepresentable instead: there is exactly one
/// duration a `Dt` can mean, so passing the wrong one is not a mistake that can
/// be written down. That is layer 0 of the ladder in `CLAUDE.md` rather than
/// layer 1, and it costs nothing.
///
/// What it is, then, is a *token* proving its holder is inside a fixed step.
/// Only [`Accumulator::pending`] mints one — the field is private to this
/// module, so even the rest of `sim` has to go through the accumulator, tests
/// included.
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

/// Turns elapsed wall clock into whole ticks.
///
/// Lives in `sim` rather than beside the frame `Clock` in `app` because it is
/// the thing that mints `Dt`, and a `Dt` minted anywhere else would be a type
/// asserting something untrue. `app` measures seconds; only this decides how
/// much simulation they are worth.
#[derive(Default)]
pub struct Accumulator {
    /// Time owed but not yet worth a whole tick. Always in `[0, Dt::SECS)`
    /// after [`pending`](Accumulator::pending) returns.
    carry: f32,
}

impl Accumulator {
    /// Absorbs one frame's elapsed seconds and yields the ticks it bought.
    ///
    /// The iterator is the only source of [`Dt`] in the program, which is what
    /// makes "the simulation only ever advances in fixed steps" a fact about
    /// the type system rather than a convention about this loop.
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

    /// How far past the last tick the accumulator sits, in seconds.
    ///
    /// Presentation only — this is what render interpolation will divide by
    /// `Dt::SECS` to get its blend factor. Nothing in `sim` may read it, or the
    /// frame rate is back inside the simulation by the side door.
    #[must_use]
    pub fn carry_secs(&self) -> f32 {
        self.carry
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

        let left = acc.carry_secs();
        assert!(left > 0.0 && left < Dt::SECS, "carry must stay a fraction of a tick, got {left}");
    }

    /// A long stall must not be repaid. See `MAX_TICKS_PER_FRAME`.
    #[test]
    fn a_stall_cannot_spiral() {
        let mut acc = Accumulator::default();

        assert_eq!(acc.pending(10.0).len() as u32, MAX_TICKS_PER_FRAME);
        assert!(
            acc.carry_secs() < Dt::SECS,
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
        assert!(acc.carry_secs().is_finite());

        assert_eq!(acc.pending(Dt::SECS).len(), 1, "the accumulator stopped working after bad input");
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
        assert_eq!(acc.carry_secs(), 0.0, "whole ticks left a remainder behind");
    }
}
