//! What a device is asking for, in **screen** terms.
//!
//! Nothing here names a key, a mouse button or a gamepad axis, for the same
//! reason nothing in `arpg-gfx` names an enemy. The binding from a physical
//! input to an [`Action`] lives in `arpg` (app), the only crate that talks to
//! winit. The simulation never sees this module's types at all — it sees
//! [`crate::Intent`], which is what these become once the camera has turned
//! "toward the top of the screen" into a world direction.
//!
//! That indirection is the entire point of an action layer, and it pays for
//! itself three times over: rebindable keys, a gamepad that pushes the same
//! actions from a different device, and a replay or an AI that synthesises
//! actions with no device behind them at all. None of those need the simulation
//! to change.

use glam::Vec2;

/// Something the player can intend.
///
/// The movement four are named in **screen** directions, not world axes,
/// because that is where the player experiences them: `MoveUp` means "toward
/// the top of the monitor". Translating that into a world direction is the
/// camera's job — see `OrthoCamera::ground_basis`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Toward the top of the screen.
    MoveUp,
    /// Toward the bottom of the screen.
    MoveDown,
    /// Toward the left of the screen.
    MoveLeft,
    /// Toward the right of the screen.
    MoveRight,
    /// Swing.
    ///
    /// Edge-triggered where the movement four are level-triggered, and that
    /// difference is the whole reason [`Actions`] carries both: holding the key
    /// must not swing every tick, and a tap shorter than a frame must still
    /// swing exactly once. `just_pressed` is the accessor; `held` is the wrong
    /// question to ask about it.
    Attack,
}

impl Action {
    /// Every action, in bit order. Iterating this is how a mask is built.
    pub const ALL: [Action; 5] =
        [Action::MoveUp, Action::MoveDown, Action::MoveLeft, Action::MoveRight, Action::Attack];

    const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// One bit per [`Action`], so the whole set is a `u32` and set operations are
/// single instructions. Edge detection in particular is one `AND NOT`.
const _: () = assert!(Action::ALL.len() <= u32::BITS as usize);

/// A set of actions.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ActionMask(u32);

impl ActionMask {
    /// No actions at all.
    pub const EMPTY: Self = Self(0);

    /// Adds an action to the set.
    pub fn insert(&mut self, action: Action) {
        self.0 |= action.bit();
    }

    /// Whether the set contains an action.
    pub fn contains(self, action: Action) -> bool {
        self.0 & action.bit() != 0
    }

    /// Actions in `self` but not in `other`.
    fn minus(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// Accumulates device input between samples.
///
/// The held set is *replaced* on every device event, while the two edge sets
/// **latch** until [`InputState::sample`] takes them. That asymmetry is
/// deliberate and it is the whole reason this type exists rather than a bare
/// `ActionMask`.
///
/// Recomputing edges at sample time instead — comparing this frame's held set
/// against last frame's — loses any press that begins *and ends* between two
/// samples. At 60Hz that is a 16ms window, which is comfortably inside a human
/// tap, so the input the player is most sure they gave is exactly the one
/// dropped. Latching is also the shape input buffering wants later: a buffer is
/// this, with a timestamp and a longer expiry.
#[derive(Default)]
pub struct InputState {
    held: ActionMask,
    pressed: ActionMask,
    released: ActionMask,
}

impl InputState {
    /// Replaces the held set, deriving the edges from what changed.
    ///
    /// Takes the whole set rather than individual press/release calls because
    /// the caller is the only thing that knows how devices map onto actions —
    /// and in particular that two keys bound to one action must not have the
    /// first release cancel the second key's hold.
    pub fn set_held(&mut self, held: ActionMask) {
        self.pressed.0 |= held.minus(self.held).0;
        self.released.0 |= self.held.minus(held).0;
        self.held = held;
    }

    /// Level-triggered state, read **without** consuming anything.
    ///
    /// The counterpart to [`InputState::sample`], and the split is the whole
    /// point: an edge must be consumed exactly once, so only `sample` may see
    /// one; held state is a fact about right now, so anything may read it any
    /// number of times. Presentation — the camera's lead, a HUD — uses this,
    /// which is what stops the renderer from eating a keypress.
    pub fn held(&self) -> ActionMask {
        self.held
    }

    /// Takes one sample of intent and clears the latched edges.
    ///
    /// Clearing happens *here*, in the only reader, for the reason the instance
    /// buffer resets inside `sink()`: "remember to clear the edges afterwards"
    /// is a rule that gets forgotten, and the symptom — one keypress firing an
    /// attack every frame until the next one — points nowhere near the cause.
    ///
    /// **Call this once per simulation tick, not once per frame.** Under a
    /// fixed timestep a frame runs zero, one or several ticks; sampling per
    /// frame means a frame that runs no ticks consumes an edge and throws it
    /// away, and uncapped that is most frames. Sampling per tick makes a press
    /// wait for a tick to consume it, and gives it to exactly one.
    pub fn sample(&mut self) -> Actions {
        let actions =
            Actions { held: self.held, pressed: self.pressed, released: self.released };
        self.pressed = ActionMask::EMPTY;
        self.released = ActionMask::EMPTY;
        actions
    }
}

/// One sample of intent: what is held, and what changed since the last sample.
///
/// A plain `Copy` value rather than a borrow of [`InputState`], so a consumer
/// cannot mutate the accumulator, cannot hold it across frames, and cannot skip
/// the clear.
#[derive(Clone, Copy, Default, Debug)]
pub struct Actions {
    held: ActionMask,
    pressed: ActionMask,
    released: ActionMask,
}

impl Actions {
    /// Level-triggered: is the action active right now. This is what continuous
    /// things — movement, blocking, channelling — ask.
    pub fn held(self, action: Action) -> bool {
        self.held.contains(action)
    }

    /// Edge-triggered: did the action begin since the last sample. This is what
    /// discrete things — attack, dodge, jump — ask, and it is true exactly once
    /// per physical press even if the press was shorter than a frame.
    pub fn just_pressed(self, action: Action) -> bool {
        self.pressed.contains(action)
    }

    /// Edge-triggered: did the action end since the last sample. Releases
    /// matter for anything charged — hold to wind up, release to swing.
    pub fn just_released(self, action: Action) -> bool {
        self.released.contains(action)
    }

    /// Movement intent in **screen** space: `x` is right, `y` is up, each in
    /// `-1..=1`.
    ///
    /// Opposite directions cancel rather than one winning, which is the
    /// behaviour that makes rolling a thumb across two keys feel like a stop
    /// instead of a lurch.
    pub fn move_axis(self) -> Vec2 {
        self.held.move_axis()
    }
}

impl ActionMask {
    /// The movement axis this set of held actions describes.
    ///
    /// On the *mask* rather than on [`Actions`], because movement is purely
    /// level-triggered: it asks only what is held, never what changed. That is
    /// what lets presentation read it without consuming an edge — see
    /// [`InputState::held`].
    pub fn move_axis(self) -> Vec2 {
        let axis = |neg, pos| match (self.contains(neg), self.contains(pos)) {
            (true, false) => -1.0,
            (false, true) => 1.0,
            _ => 0.0,
        };
        Vec2::new(
            axis(Action::MoveLeft, Action::MoveRight),
            axis(Action::MoveDown, Action::MoveUp),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held_of(actions: &[Action]) -> ActionMask {
        let mut mask = ActionMask::EMPTY;
        for a in actions {
            mask.insert(*a);
        }
        mask
    }

    /// The case an edge model recomputed at sample time gets wrong: a press
    /// that begins and ends inside one frame must still be seen exactly once.
    #[test]
    fn a_tap_shorter_than_a_frame_survives() {
        let mut input = InputState::default();
        input.set_held(held_of(&[Action::MoveUp]));
        input.set_held(ActionMask::EMPTY);

        let actions = input.sample();
        assert!(actions.just_pressed(Action::MoveUp));
        assert!(actions.just_released(Action::MoveUp));
        assert!(!actions.held(Action::MoveUp), "the key is no longer down");
    }

    /// **A frame that runs no ticks must not eat a keypress.**
    ///
    /// Under a fixed timestep a frame buys zero, one or several ticks, and
    /// uncapped it is usually zero. `sample` clears the latched edges, so
    /// calling it once per *frame* would consume a press on a frame that
    /// simulated nothing and throw it away. Presentation therefore reads
    /// `held`, which consumes nothing, and only a tick calls `sample`.
    ///
    /// The symptom this prevents is "the attack sometimes does not come out",
    /// which points nowhere near the frame loop.
    #[test]
    fn reading_held_state_never_consumes_an_edge() {
        let mut input = InputState::default();
        input.set_held(held_of(&[Action::MoveRight]));

        // Several frames go by drawing, none of them running a tick.
        for _ in 0..5 {
            assert!(input.held().contains(Action::MoveRight));
        }

        // The tick that finally runs still sees the press exactly once.
        assert!(input.sample().just_pressed(Action::MoveRight), "the press was eaten by a redraw");
        assert!(!input.sample().just_pressed(Action::MoveRight), "the press fired twice");
    }

    /// The axis is level-triggered, so reading it must not depend on which of
    /// the two doors it came through.
    #[test]
    fn the_movement_axis_is_the_same_held_or_sampled() {
        let mut input = InputState::default();
        input.set_held(held_of(&[Action::MoveRight, Action::MoveUp]));

        let peeked = input.held().move_axis();
        assert_eq!(peeked, input.sample().move_axis());
    }

    /// Edges are latched, so they must not survive the sample that consumed
    /// them — otherwise one press attacks every frame forever.
    #[test]
    fn sample_clears_edges_but_not_held() {
        let mut input = InputState::default();
        input.set_held(held_of(&[Action::MoveRight]));

        assert!(input.sample().just_pressed(Action::MoveRight));

        let second = input.sample();
        assert!(!second.just_pressed(Action::MoveRight));
        assert!(second.held(Action::MoveRight), "the key is still down");
    }

    /// Holding a direction that is already held is not a new press. Key repeat
    /// is filtered at the device layer, but an idempotent `set_held` means a
    /// second source of the same action cannot double-fire either.
    #[test]
    fn re_asserting_the_same_held_set_produces_no_edges() {
        let mut input = InputState::default();
        input.set_held(held_of(&[Action::MoveUp]));
        input.sample();
        input.set_held(held_of(&[Action::MoveUp]));

        assert!(!input.sample().just_pressed(Action::MoveUp));
    }

    #[test]
    fn opposite_directions_cancel() {
        let mut input = InputState::default();
        input.set_held(held_of(&[Action::MoveLeft, Action::MoveRight]));
        assert_eq!(input.sample().move_axis(), Vec2::ZERO);
    }

    /// The raw axis is deliberately **not** normalised — normalising is
    /// `MoveDir`'s job, at the point screen becomes world. See
    /// `a_diagonal_is_unit_length` in `intent.rs` for the other half.
    #[test]
    fn the_raw_axis_is_a_screen_axis_not_a_direction() {
        let mut input = InputState::default();
        input.set_held(held_of(&[Action::MoveUp, Action::MoveRight]));
        assert_eq!(input.sample().move_axis(), Vec2::new(1.0, 1.0));
    }
}
