//! The only place a physical key is translated into an [`Action`].
//!
//! This is the layer `arpg-core` deliberately does not have and `arpg-sim`
//! deliberately cannot reach: winit is named here and nowhere downstream.

use winit::keyboard::KeyCode;

use arpg_core::{Action, ActionMask, Actions, InputState};

/// Which physical keys mean which action.
///
/// A table rather than a `match` because bindings are *data*: a match compiles
/// the mapping into control flow, where nothing can enumerate it, print it to a
/// settings screen, or replace it at runtime. As a slice, "load bindings from a
/// file" is a change of where this array comes from and nothing else.
///
/// Note the same action appearing twice. That is the case the naive
/// implementation gets wrong — see `down` below.
const BINDINGS: &[(KeyCode, Action)] = &[
    (KeyCode::KeyW, Action::MoveUp),
    (KeyCode::KeyS, Action::MoveDown),
    (KeyCode::KeyA, Action::MoveLeft),
    (KeyCode::KeyD, Action::MoveRight),
    (KeyCode::ArrowUp, Action::MoveUp),
    (KeyCode::ArrowDown, Action::MoveDown),
    (KeyCode::ArrowLeft, Action::MoveLeft),
    (KeyCode::ArrowRight, Action::MoveRight),
];

/// `down` is a bitset over `BINDINGS`, so the table cannot outgrow it quietly.
/// Widening to `u64` is the fix if it ever does; a `Vec<bool>` is not, because
/// this is touched on the input path every frame.
const _: () = assert!(BINDINGS.len() <= u32::BITS as usize);

/// Turns winit key events into [`Actions`].
#[derive(Default)]
pub(crate) struct Input {
    /// Bit *i* is set while `BINDINGS[i]`'s key is physically down.
    ///
    /// Tracking **keys** and deriving actions from them — rather than tracking
    /// actions directly — is what makes two keys bound to one action work. With
    /// a single "is MoveUp held" flag, pressing W, then Up, then releasing W
    /// clears the flag while Up is still held: the character stops dead with a
    /// key still down. Refcounting per action fixes the symptom but drifts out
    /// of sync the first time an event is missed. Recomputing from the physical
    /// truth cannot drift, because there is only one copy of it.
    down: u32,
    state: InputState,
}

impl Input {
    /// Feeds one key event in. Unbound keys are ignored, which is what leaves
    /// the debug keys in `app.rs` free to handle themselves.
    pub(crate) fn on_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        // Key repeat is text entry leaking into a game loop: hold a key and the
        // OS invents a stream of presses at its own typematic rate. Held state
        // already covers "still down", so a repeat carries no information — and
        // letting it through would make `just_pressed` fire over and over for
        // one physical press, turning a tap on the attack key into a machine
        // gun whose rate is a keyboard setting.
        if repeat {
            return;
        }

        for (i, (bound, _)) in BINDINGS.iter().enumerate() {
            if *bound == key {
                let bit = 1u32 << i;
                if pressed {
                    self.down |= bit;
                } else {
                    self.down &= !bit;
                }
            }
        }
        self.sync();
    }

    /// Releases everything.
    ///
    /// Call this when the window loses focus. Key-up is delivered to whoever
    /// has focus, so alt-tabbing mid-stride means the release never arrives and
    /// the character keeps running until the window is focused and that key is
    /// pressed and released again. It looks like a physics bug and it is a
    /// bookkeeping one.
    pub(crate) fn release_all(&mut self) {
        self.down = 0;
        self.sync();
    }

    /// Recomputes the action set from the keys actually down. Cheap enough to
    /// do on every event — the table is eight entries — and the only way the
    /// derived state can be wrong is if the physical state is.
    fn sync(&mut self) {
        let mut held = ActionMask::EMPTY;
        for (i, (_, action)) in BINDINGS.iter().enumerate() {
            if self.down & (1u32 << i) != 0 {
                held.insert(*action);
            }
        }
        self.state.set_held(held);
    }

    /// One sample of intent, for one tick. Clears the latched edges.
    pub(crate) fn sample(&mut self) -> Actions {
        self.state.sample()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug the key-level bitset exists to prevent.
    #[test]
    fn releasing_one_of_two_keys_bound_to_the_same_action_keeps_it_held() {
        let mut input = Input::default();
        input.on_key(KeyCode::KeyW, true, false);
        input.on_key(KeyCode::ArrowUp, true, false);
        input.on_key(KeyCode::KeyW, false, false);

        assert!(input.sample().held(Action::MoveUp), "Up is still down");

        input.on_key(KeyCode::ArrowUp, false, false);
        assert!(!input.sample().held(Action::MoveUp));
    }

    /// One physical press is one press, however long it is held.
    #[test]
    fn key_repeat_does_not_produce_extra_presses() {
        let mut input = Input::default();
        input.on_key(KeyCode::KeyD, true, false);
        assert!(input.sample().just_pressed(Action::MoveRight));

        for _ in 0..10 {
            input.on_key(KeyCode::KeyD, true, true);
        }
        let actions = input.sample();
        assert!(!actions.just_pressed(Action::MoveRight));
        assert!(actions.held(Action::MoveRight), "repeat must not release it either");
    }

    /// Losing focus mid-stride must not leave the character running.
    #[test]
    fn losing_focus_releases_everything() {
        let mut input = Input::default();
        input.on_key(KeyCode::KeyW, true, false);
        input.on_key(KeyCode::KeyD, true, false);
        input.release_all();

        assert_eq!(input.sample().move_axis(), glam::Vec2::ZERO);
    }

    /// Unbound keys must fall through untouched, so the debug keys keep working.
    #[test]
    fn an_unbound_key_changes_nothing() {
        let mut input = Input::default();
        input.on_key(KeyCode::KeyV, true, false);
        assert_eq!(input.down, 0);
    }
}
