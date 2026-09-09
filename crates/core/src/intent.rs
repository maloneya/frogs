//! What one tick of input *means* to the simulation, once the camera has had
//! its say.
//!
//! The other half of the seam from [`crate::Action`]: an action is named in
//! screen directions, because that is where the player experiences it, and
//! resolving screen to world is the camera's business. `app` does that
//! resolution and hands the simulation an [`Intent`] — which names no key, no
//! screen and no camera.

use glam::Vec3;

/// A horizontal world-space direction of travel: unit length, or exactly zero.
///
/// A newtype rather than a bare `Vec3` because "normalise the input vector" is
/// a rule everyone forgets exactly once, and the symptom is subtle enough to
/// ship: holding two keys moves you √2 ≈ 1.41 times faster than holding one, so
/// the fastest way across the arena is permanently diagonal. Doing it at the
/// only constructor means no caller can be the one who forgets — including the
/// analog stick that arrives later and does not clamp itself.
///
/// Horizontal because the ground plane is where movement happens; letting a Y
/// component through would have the character walk into the floor.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct MoveDir(Vec3);

impl MoveDir {
    /// Standing still.
    pub const NONE: Self = Self(Vec3::ZERO);

    /// The only door: flattens onto the ground plane, then normalises.
    ///
    /// `normalize_or_zero` rather than `normalize`, because the zero vector is
    /// the common case — nobody is pressing anything — and normalising it
    /// yields NaN, which then propagates into a position that no clamp can
    /// recover.
    pub fn new(v: Vec3) -> Self {
        Self(Vec3::new(v.x, 0.0, v.z).normalize_or_zero())
    }

    /// The direction as a vector, for whoever is doing the integrating.
    pub fn as_vec3(self) -> Vec3 {
        self.0
    }
}

/// One tick's worth of intent, in the **simulation's** vocabulary.
///
/// The seam between `app` and `sim`, and it exists because [`crate::Actions`] cannot
/// be that seam: the movement actions are named in *screen* directions, and
/// which world direction "up" means is the camera's business. Handing `Actions`
/// to the simulation would put a presentation decision inside it.
///
/// So `app` resolves screen to world, and this is what comes out the other
/// side: a world-space direction and the discrete things the player asked for
/// this tick. Adding an intent later — dodge, block — adds a field here rather
/// than a parameter to `World::step`, which is what stops that signature
/// growing a tail of booleans nobody can read at the call site.
#[derive(Clone, Copy, Default, Debug)]
pub struct Intent {
    move_dir: MoveDir,
    attack: bool,
}

impl Intent {
    /// Nothing at all. What a tick with no input looks like.
    pub const NONE: Self = Self { move_dir: MoveDir::NONE, attack: false };

    /// Builds one tick's intent.
    ///
    /// `attack` is an **edge**: true on the tick the swing was asked for, not
    /// while a key is held. Passing `held` here would swing every tick the
    /// button is down, which is the bug the `pressed`/`held` split in
    /// [`crate::Actions`] exists to make hard.
    #[must_use]
    pub fn new(move_dir: MoveDir, attack: bool) -> Self {
        Self { move_dir, attack }
    }

    /// Where the player is trying to go, in world space.
    #[must_use]
    pub fn move_dir(self) -> MoveDir {
        self.move_dir
    }

    /// Whether a swing was asked for on this tick.
    #[must_use]
    pub fn attack(self) -> bool {
        self.attack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug the newtype exists to prevent: diagonals must not be faster.
    /// A screen axis of `(1, 1)` is √2 long; what reaches the simulation is not.
    #[test]
    fn a_diagonal_is_unit_length() {
        let dir = MoveDir::new(Vec3::new(1.0, 0.0, -1.0));
        assert!((dir.as_vec3().length() - 1.0).abs() < 1e-6);
    }

    /// Standing still must stay exactly zero, not NaN.
    #[test]
    fn no_input_is_no_movement() {
        assert_eq!(MoveDir::new(Vec3::ZERO).as_vec3(), Vec3::ZERO);
    }

    /// Any vertical component is dropped, so movement cannot leave the ground
    /// plane or lose length to a Y term.
    #[test]
    fn move_dir_is_flattened_before_normalising() {
        let dir = MoveDir::new(Vec3::new(0.0, 99.0, 2.0));
        assert_eq!(dir.as_vec3(), Vec3::new(0.0, 0.0, 1.0));
    }

    /// A swing is an edge, so `Intent::NONE` must not be asking for one.
    #[test]
    fn nothing_at_all_asks_for_nothing() {
        assert_eq!(Intent::NONE.move_dir(), MoveDir::NONE);
        assert!(!Intent::NONE.attack());
    }
}
