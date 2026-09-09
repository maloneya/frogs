//! Where a swing goes, and the hitbox generated from it.
//!
//! An attack is configured as two positions — where the hitbox starts and where
//! it finishes, in the player's own frame — and the hitbox is *generated* from
//! them: one disc per tick of the active window. A stab, an arc and a slam are
//! then three settings of the same two points rather than three kinds of thing,
//! which is the same argument [`crate::pass::source`] makes about cadence and
//! placement: independent axes beat an enum of every useful combination,
//! because the combination nobody enumerated is the one someone wants.
//!
//! ## Why the hitbox is a value
//!
//! The generated hitbox is stored, not recomputed. That is the whole point of
//! this module existing: [`crate::pass::attack`] tests the disc for the tick it
//! is on, and [`crate::World::extract`] draws every disc — both *placing* discs
//! this type already decided rather than each working out where the sword is.
//!
//! Two computations of one formula can disagree, and this is the worst place
//! for them to. A swing that draws in one place and hits in another does not
//! crash, does not fail a shape test, and looks entirely plausible in a
//! screenshot; the only symptom is that the game feels wrong, and feel is the
//! one thing being tuned here. Two readers of one value cannot drift.
//!
//! ## Why the interpolation is polar
//!
//! Interpolating the two configured points *cartesianly* cannot make an arc.
//! It cuts the chord: the disc travels in a straight line between the ends and
//! passes closer to the player in the middle than at either end. That is the
//! swing shape you would most want and the one that would come out wrong —
//! wrong by a fraction of a unit, in the middle of a short active window, which is
//! not something any screenshot shows.
//!
//! So a disc holds an angle off the facing and a distance from the player, and
//! the two interpolate separately. Ends on the same ray degenerate to the
//! straight line a stab wants; ends at the same distance sweep a real arc. The
//! configuration stays cartesian because "the sword starts here and ends there"
//! is something you can picture, and a pair of radians is not.

use glam::{FloatExt, Vec2};

use crate::angle;
use crate::hash::Fnv;

/// One disc of a hitbox, in the player's frame.
///
/// **Polar rather than cartesian, and that is not a storage detail.** Placing
/// it in the world is then `facing + angle` handed to the single `(sin, cos)`
/// convention this codebase has, instead of a right-hand basis vector — and a
/// mirrored basis is precisely the class of mistake that compiles, validates,
/// draws, and swings out of the character's back in silence.
#[derive(Clone, Copy)]
pub(crate) struct Disc {
    /// Off the facing direction, in radians. Zero is straight ahead.
    angle: f32,
    /// From the player's centre, in world units.
    distance: f32,
    /// The disc's own radius.
    radius: f32,
}

impl Disc {
    /// Where this disc sits in the world, and how big it is.
    ///
    /// Yaw 0 faces `+Z` and positive turns toward `+X` — the convention
    /// `Instance::with_yaw` and `shader.wgsl` share. Because the disc is stored
    /// as an angle, honouring that convention here is one addition rather than
    /// a basis someone has to get the signs right on.
    pub(crate) fn place(self, origin: Vec2, facing: f32) -> (Vec2, f32) {
        let (sin, cos) = (facing + self.angle).sin_cos();
        (origin + Vec2::new(sin, cos) * self.distance, self.radius)
    }
}

/// A swing's shape: where its hitbox starts, where it ends, and how big it is
/// at each end.
///
/// The two ends are offsets in the player's frame, `x` to the right of the
/// facing and `y` straight along it — the local twin of the ground-plane
/// convention [`crate::on_ground`] spells out.
///
/// The radius is a pair for the same reason the position is: without it a slam
/// is a hitbox that does not move, which is a slam with no impact to it. Three
/// dials, and the named swings fall out of them — an arc varies the angle, a
/// stab the distance, a slam the radius, and nothing stops one swing varying
/// all three.
pub(crate) struct Swing {
    start: Vec2,
    end: Vec2,
    radius: (f32, f32),
}

impl Swing {
    /// Describes a swing. `const`, so every check below is a compile error at
    /// the definition site rather than a panic on the tick someone swings.
    pub(crate) const fn new(start: Vec2, end: Vec2, radius: (f32, f32)) -> Self {
        // A hitbox centred on the player has no direction to be placed along,
        // and its angle is whatever `atan2(0, 0)` happens to return. Cheap to
        // rule out here, and the alternative is a swing that mysteriously
        // points north.
        assert!(start.x != 0.0 || start.y != 0.0, "a swing starting on the player has no direction");
        assert!(end.x != 0.0 || end.y != 0.0, "a swing ending on the player has no direction");
        assert!(radius.0 > 0.0 && radius.1 > 0.0, "a hitbox with no radius can never touch anything");

        Self { start, end, radius }
    }

    /// Generates the hitbox: one disc per tick of the active window.
    ///
    /// `N` is that window's length in ticks, so the samples land exactly on the
    /// ticks that will test them — the first on `start`, the last on `end`.
    ///
    /// **The hitbox's spatial resolution is therefore the tick rate.** That is
    /// fine while the two ends are close together and stops being fine once a
    /// swing travels far in a few ticks: consecutive discs stop overlapping,
    /// and a body standing in the gap between two of them is passed straight
    /// through. It is a real limit, it cannot bite until a swing actually
    /// moves, and it belongs to the change that makes one move.
    pub(crate) fn generate<const N: usize>(&self, samples: usize) -> Hitbox<N> {
        assert!(samples > 0 && samples <= N, "active window must fit the hitbox storage");
        let (from_angle, from_distance) = polar(self.start);
        let (to_angle, to_distance) = polar(self.end);

        // The short way round, for the same reason `blend_angle` needs it: a
        // swing whose ends straddle the `±PI` branch cut is a small turn that
        // naive subtraction reports as an almost-full revolution the other way.
        let turn = angle::shortest_arc(from_angle, to_angle);

        Hitbox {
            discs: core::array::from_fn(|i| {
                // Inclusive of both ends, so the last tick lands *on* `end` and
                // the configuration means what it says. A one-tick window is
                // its own start, since there is nowhere to travel to.
                let t = if samples > 1 {
                    i.min(samples - 1) as f32 / (samples - 1) as f32
                } else {
                    0.0
                };

                Disc {
                    angle: angle::wrap(from_angle + turn * t),
                    distance: from_distance.lerp(to_distance, t),
                    radius: self.radius.0.lerp(self.radius.1, t),
                }
            }),
            len: samples,
        }
    }
}

/// A swing's hitbox: where it is on each tick of its active window.
///
/// **The value both readers share.** See the module docs for why it is stored
/// rather than recomputed.
pub(crate) struct Hitbox<const N: usize> {
    discs: [Disc; N],
    len: usize,
}

impl<const N: usize> Hitbox<N> {
    /// The disc live on tick `i` of the active window.
    pub(crate) fn at(&self, i: usize) -> Disc {
        self.discs[i]
    }

    /// Every disc the swing occupies, in the order it occupies them.
    pub(crate) fn discs(&self) -> &[Disc] {
        &self.discs[..self.len]
    }

    /// Feeds the hitbox into the world hash.
    ///
    /// The hitbox is the difference between two configurations, and a hash that
    /// skipped it would let a replay swing a different shape and call that
    /// agreement.
    pub(crate) fn hash(&self, h: &mut Fnv) {
        h.usize(self.len);
        // Hash the reserved tail as well as the live prefix. It is derived and
        // cannot affect behavior, but the replay rule is deliberately every
        // stored field, including derived state.
        for &Disc { angle, distance, radius } in &self.discs {
            h.f32(angle);
            h.f32(distance);
            h.f32(radius);
        }
    }
}

/// Splits a local offset into the angle and distance the interpolation works in.
///
/// `x.atan2(y)`, not the textbook `y.atan2(x)`: straight ahead is local `+y`
/// here and must come out as an angle of zero, which is exactly the rotation
/// [`Disc::place`] undoes.
fn polar(offset: Vec2) -> (f32, f32) {
    (offset.x.atan2(offset.y), offset.length())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: usize = 4;

    fn placed<const N: usize>(hitbox: &Hitbox<N>) -> Vec<(Vec2, f32)> {
        hitbox.discs().iter().map(|d| d.place(Vec2::ZERO, 0.0)).collect()
    }

    /// The configuration means what it says: the window opens on `start` and
    /// closes on `end`, with nothing clipped off either edge.
    #[test]
    fn the_ends_land_on_the_configured_positions() {
        let swing = Swing::new(Vec2::new(-0.9, 0.7), Vec2::new(0.9, 0.7), (0.3, 0.5));
        let discs = placed(&swing.generate::<SAMPLES>(SAMPLES));

        let (first, first_radius) = discs[0];
        let (last, last_radius) = discs[SAMPLES - 1];

        assert!(first.distance(Vec2::new(-0.9, 0.7)) < 1e-5, "the first disc is not at `start`");
        assert!(last.distance(Vec2::new(0.9, 0.7)) < 1e-5, "the last disc is not at `end`");
        assert!((first_radius - 0.3).abs() < 1e-6);
        assert!((last_radius - 0.5).abs() < 1e-6);
    }

    /// **The reason the interpolation is polar, stated as a test.**
    ///
    /// Both ends sit at the same distance from the player, so an arc keeps that
    /// distance the whole way round. A cartesian lerp between the same two
    /// points cuts the chord and brings the middle discs ~0.24 units closer,
    /// which is a quarter of the reach — a hit that lands early against a body
    /// in front and no visible defect anywhere.
    #[test]
    fn an_arc_sweeps_rather_than_cutting_the_chord() {
        let end = Vec2::new(0.9, 0.7);
        let reach = end.length();
        let swing = Swing::new(Vec2::new(-0.9, 0.7), end, (0.6, 0.6));

        for (i, (centre, _)) in
            placed(&swing.generate::<SAMPLES>(SAMPLES)).into_iter().enumerate()
        {
            let d = centre.length();
            assert!((d - reach).abs() < 1e-5, "disc {i} sits at {d}, off the arc at {reach}");
        }
    }

    /// The other degenerate case, and the one a stab wants: ends on the same
    /// ray give a straight line out, with every disc dead ahead.
    #[test]
    fn a_stab_runs_straight_out_along_the_facing() {
        let swing = Swing::new(Vec2::new(0.0, 0.5), Vec2::new(0.0, 1.9), (0.3, 0.3));
        let mut last = 0.0;

        for (i, (centre, _)) in
            placed(&swing.generate::<SAMPLES>(SAMPLES)).into_iter().enumerate()
        {
            assert!(centre.x.abs() < 1e-6, "disc {i} drifted off the facing axis");
            assert!(centre.y > last, "disc {i} did not advance");
            last = centre.y;
        }
    }

    /// A swing whose ends coincide tests the same disc every tick. It has to
    /// survive generation unchanged, since it is also the default configuration.
    #[test]
    fn a_swing_that_does_not_move_generates_one_position() {
        let at = Vec2::new(0.0, 1.1);
        let swing = Swing::new(at, at, (0.6, 0.6));

        for (i, (centre, radius)) in
            placed(&swing.generate::<SAMPLES>(SAMPLES)).into_iter().enumerate()
        {
            assert!(centre.distance(at) < 1e-6, "disc {i} moved");
            assert!((radius - 0.6).abs() < 1e-6, "disc {i} resized");
        }
    }

    /// The hitbox is in the player's frame, so turning carries it round. The
    /// mistake this catches is a mirrored placement, which at facing 0 is
    /// indistinguishable from a correct one.
    #[test]
    fn the_hitbox_turns_with_the_player() {
        let swing = Swing::new(Vec2::new(0.9, 0.7), Vec2::new(0.9, 0.7), (0.6, 0.6));
        let disc = swing.generate::<1>(1).at(0);

        // Facing +X: the local offset (right 0.9, ahead 0.7) must come out
        // ahead in +X and to the *right* of that, which is -Z.
        let (centre, _) = disc.place(Vec2::ZERO, core::f32::consts::FRAC_PI_2);
        assert!((centre.x - 0.7).abs() < 1e-6, "the ahead component did not follow the facing");
        assert!((centre.y + 0.9).abs() < 1e-6, "the sideways component is mirrored");
    }
}
