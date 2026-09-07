//! The only place a physical key is translated into an [`Action`].
//!
//! This is the layer `arpg-core` deliberately does not have and `arpg-sim`
//! deliberately cannot reach: winit is named here and nowhere downstream.

use winit::keyboard::KeyCode;

use crate::ui::{Menu, MenuKey};
use arpg_core::{Action, ActionMask, Actions, InputState};
use arpg_sim::RecoveryTicks;

/// Which physical keys mean which action, and what each key is called.
///
/// A table rather than a `match` because bindings are *data*: a match compiles
/// the mapping into control flow, where nothing can enumerate it, print it to a
/// settings screen, or replace it at runtime. As a slice, "load bindings from a
/// file" is a change of where this array comes from and nothing else.
///
/// The name column is what the test harness types. It lives *here*, rather than
/// in a lookup table beside the harness, so the two cannot drift: a second table
/// would have to be remembered every time a key is bound, and forgetting it
/// would leave the new key undrivable — the tooling silently falling behind the
/// game it is meant to test. One table means a binding is testable the moment it
/// exists.
///
/// Note the same action appearing twice. That is the case the naive
/// implementation gets wrong — see `down` below.
// Both contexts share key names, while only the game column can mint Actions.
type Binding = (&'static str, KeyCode, Option<Action>, Option<MenuKey>);
const BINDINGS: &[Binding] = &[
    ("w", KeyCode::KeyW, Some(Action::MoveUp), None),
    ("s", KeyCode::KeyS, Some(Action::MoveDown), None),
    ("a", KeyCode::KeyA, Some(Action::MoveLeft), None),
    ("d", KeyCode::KeyD, Some(Action::MoveRight), None),
    ("up", KeyCode::ArrowUp, Some(Action::MoveUp), None),
    ("down", KeyCode::ArrowDown, Some(Action::MoveDown), None),
    ("left", KeyCode::ArrowLeft, Some(Action::MoveLeft), Some(MenuKey::Decrease)),
    ("right", KeyCode::ArrowRight, Some(Action::MoveRight), Some(MenuKey::Increase)),
    ("space", KeyCode::Space, Some(Action::Attack), None),
    ("f1", KeyCode::F1, None, Some(MenuKey::Toggle)),
    ("escape", KeyCode::Escape, None, Some(MenuKey::Close)),
    ("r", KeyCode::KeyR, None, Some(MenuKey::Reset)),
];

/// The one input route used by window events and injected harness keys.
/// Context changes discard held gameplay and latched edges; a key held across
/// a transition must be released and pressed again before it can act.
#[derive(Default)]
pub(crate) struct Controls {
    state: InputState,
    /// Keys accepted by gameplay. Separate from physical state so a held key
    /// swallowed by the menu cannot reappear when the menu closes.
    accepted: u32,
    menu: Menu,
    /// Physical keys, including those swallowed by the menu.
    down: u32,
}

impl Controls {
    /// Returns whether the event was handled, including modal unbound keys and
    /// duplicate presses. Only unhandled events may reach app debug shortcuts.
    pub(crate) fn on_key(
        &mut self,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
        recovery: RecoveryTicks,
    ) -> bool {
        let was_open = self.menu.open();
        let binding = BINDINGS.iter().enumerate().find(|(_, (_, bound, ..))| *bound == key);
        let menu_key = binding.and_then(|(_, (_, _, _, menu))| *menu);
        let consumed = was_open || matches!(menu_key, Some(MenuKey::Toggle));
        if repeat {
            return consumed;
        }
        let Some((index, _)) = binding else { return consumed };
        let bit = 1 << index;
        if pressed {
            if self.down & bit != 0 {
                return true; // A duplicate press has no effect in either context.
            }
            self.down |= bit;
        } else {
            self.down &= !bit;
        }
        if pressed && let Some(menu_key) = menu_key {
            self.menu.on_key(menu_key, recovery);
        }
        if was_open != self.menu.open() {
            self.accepted = 0;
            self.state = InputState::default();
        } else if !consumed {
            if pressed {
                self.accepted |= bit;
            } else {
                self.accepted &= !bit;
            }
            self.sync();
        }
        consumed
    }

    /// Focus loss discards held keys and unsampled edges, but preserves the menu.
    pub(crate) fn release_all(&mut self) {
        self.down = 0;
        self.accepted = 0;
        self.state = InputState::default();
    }

    /// Sample once per simulation tick; zero-tick frames must retain edges.
    pub(crate) fn sample(&mut self) -> Actions {
        self.state.sample()
    }

    /// Presentation can read held state without consuming an input edge.
    pub(crate) fn held(&self) -> ActionMask {
        self.state.held()
    }

    /// Derive actions from accepted keys, so releasing W cannot stop movement
    /// while Up is still held. The table costs only a handful of comparisons.
    fn sync(&mut self) {
        let mut held = ActionMask::EMPTY;
        for (i, (_, _, action, _)) in BINDINGS.iter().enumerate() {
            if self.accepted & (1u32 << i) != 0
                && let Some(action) = action
            {
                held.insert(*action);
            }
        }
        self.state.set_held(held);
    }

    pub(crate) fn menu(&self) -> &Menu {
        &self.menu
    }

    pub(crate) fn take_recovery(&mut self) -> Option<RecoveryTicks> {
        self.menu.take_recovery()
    }
}

/// The key a harness command names, if it is bound to anything.
pub(crate) fn key_named(name: &str) -> Option<KeyCode> {
    BINDINGS.iter().find(|(n, ..)| *n == name).map(|&(_, key, ..)| key)
}

/// Every bound key's name, for error messages that tell the caller what it
/// *could* have said.
pub(crate) fn key_names() -> impl Iterator<Item = &'static str> {
    BINDINGS.iter().map(|&(name, ..)| name)
}

/// `down` is a bitset over `BINDINGS`, so the table cannot outgrow it quietly.
/// Widening to `u64` is the fix if it ever does; a `Vec<bool>` is not, because
/// this is touched on the input path every frame.
const _: () = assert!(BINDINGS.len() <= u32::BITS as usize);

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(controls: &mut Controls, name: &str) {
        let key = key_named(name).expect("every UI key is harness-drivable");
        controls.on_key(key, true, false, RecoveryTicks::default());
        controls.on_key(key, false, false, RecoveryTicks::default());
    }

    #[test]
    fn modal_transitions_discard_edges_and_require_fresh_gameplay_presses() {
        let mut controls = Controls::default();
        let recovery = RecoveryTicks::default();
        controls.on_key(KeyCode::Space, true, false, recovery);
        controls.on_key(KeyCode::KeyW, true, false, recovery);
        // Open before any tick has consumed the attack edge.
        tap(&mut controls, "f1");
        assert!(controls.menu().open());
        let actions = controls.sample();
        assert!(!actions.just_pressed(Action::Attack));
        assert_eq!(actions.move_axis(), glam::Vec2::ZERO);
        tap(&mut controls, "right");
        tap(&mut controls, "space");
        assert!(!controls.sample().just_pressed(Action::Attack));
        assert_eq!(controls.held().move_axis(), glam::Vec2::ZERO);

        let esc = key_named("escape").unwrap();
        assert!(controls.on_key(esc, true, false, recovery), "closing must consume Escape");
        assert!(!controls.menu().open());
        assert!(controls.on_key(esc, true, false, recovery), "a duplicate must not reach quit");
        // A held W cannot start moving through repeat OR duplicate injection.
        controls.on_key(KeyCode::KeyW, true, true, recovery);
        controls.on_key(KeyCode::KeyW, true, false, recovery);
        assert_eq!(controls.sample().move_axis(), glam::Vec2::ZERO);
        controls.on_key(KeyCode::KeyW, false, false, recovery);
        controls.on_key(KeyCode::KeyW, true, false, recovery);
        assert!(controls.sample().held(Action::MoveUp));
        tap(&mut controls, "space");
        assert!(controls.sample().just_pressed(Action::Attack));
        assert!(!controls.sample().just_pressed(Action::Attack));
    }

    #[test]
    fn edits_coalesce_until_consumed_and_survive_closing_the_menu() {
        let mut controls = Controls::default();
        tap(&mut controls, "f1");
        tap(&mut controls, "right");
        tap(&mut controls, "right");
        assert_eq!(controls.menu().pending().unwrap().get(), 12);
        // Readout / gameplay sampling on zero-tick frames must not eat an edit.
        let _ = controls.menu().recovery(RecoveryTicks::default());
        let _ = controls.sample();
        tap(&mut controls, "escape");
        assert_eq!(controls.take_recovery().unwrap().get(), 12);
        assert!(controls.take_recovery().is_none());
        tap(&mut controls, "f1");
        tap(&mut controls, "left");
        tap(&mut controls, "r");
        assert_eq!(controls.take_recovery(), Some(RecoveryTicks::default()));
    }

    #[test]
    fn stepper_respects_sim_bounds_and_ignores_os_repeat() {
        let mut controls = Controls::default();
        tap(&mut controls, "f1");
        controls.on_key(KeyCode::ArrowRight, true, true, RecoveryTicks::default());
        assert!(controls.menu().pending().is_none());
        for _ in 0..RecoveryTicks::MAX + 5 {
            tap(&mut controls, "left");
        }
        assert_eq!(controls.menu().pending().unwrap().get(), RecoveryTicks::MIN);
        for _ in 0..RecoveryTicks::MAX + 5 {
            tap(&mut controls, "right");
        }
        assert_eq!(controls.menu().pending().unwrap().get(), RecoveryTicks::MAX);
    }

    #[test]
    fn focus_loss_discards_an_unsampled_attack_and_held_movement() {
        let mut controls = Controls::default();
        tap(&mut controls, "space");
        controls.on_key(KeyCode::KeyD, true, false, RecoveryTicks::default());
        controls.release_all();
        let actions = controls.sample();
        assert!(!actions.just_pressed(Action::Attack));
        assert_eq!(actions.move_axis(), glam::Vec2::ZERO);
    }

    /// The bug the key-level bitset exists to prevent.
    #[test]
    fn releasing_one_of_two_keys_bound_to_the_same_action_keeps_it_held() {
        let mut input = Controls::default();
        input.on_key(KeyCode::KeyW, true, false, RecoveryTicks::default());
        input.on_key(KeyCode::ArrowUp, true, false, RecoveryTicks::default());
        input.on_key(KeyCode::KeyW, false, false, RecoveryTicks::default());

        assert!(input.sample().held(Action::MoveUp), "Up is still down");

        input.on_key(KeyCode::ArrowUp, false, false, RecoveryTicks::default());
        assert!(!input.sample().held(Action::MoveUp));
    }

    /// One physical press is one press, however long it is held.
    #[test]
    fn key_repeat_does_not_produce_extra_presses() {
        let mut input = Controls::default();
        input.on_key(KeyCode::KeyD, true, false, RecoveryTicks::default());
        assert!(input.sample().just_pressed(Action::MoveRight));

        for _ in 0..10 {
            input.on_key(KeyCode::KeyD, true, true, RecoveryTicks::default());
        }
        let actions = input.sample();
        assert!(!actions.just_pressed(Action::MoveRight));
        assert!(actions.held(Action::MoveRight), "repeat must not release it either");
    }

    /// Losing focus mid-stride must not leave the character running.
    #[test]
    fn losing_focus_releases_everything() {
        let mut input = Controls::default();
        input.on_key(KeyCode::KeyW, true, false, RecoveryTicks::default());
        input.on_key(KeyCode::KeyD, true, false, RecoveryTicks::default());
        input.release_all();

        assert_eq!(input.sample().move_axis(), glam::Vec2::ZERO);
    }

    /// The harness drives the game by these names, so every binding must have
    /// exactly one, and no two may collide — a duplicate would silently shadow
    /// whichever key came second in the table.
    #[test]
    fn every_binding_has_a_unique_name() {
        let mut seen = std::collections::HashSet::new();
        for (name, key, ..) in BINDINGS {
            assert!(!name.is_empty(), "{key:?} has no name");
            assert!(seen.insert(*name), "duplicate name {name:?}");
            assert_eq!(key_named(name), Some(*key), "{name:?} does not resolve back");
        }
        assert_eq!(seen.len(), key_names().count());
    }

    #[test]
    fn an_unknown_name_is_not_a_key() {
        assert_eq!(key_named("q"), None);
        assert_eq!(key_named(""), None);
    }

    /// Unbound keys must fall through untouched, so the debug keys keep working.
    #[test]
    fn an_unbound_key_changes_nothing() {
        let mut input = Controls::default();
        input.on_key(KeyCode::KeyV, true, false, RecoveryTicks::default());
        assert_eq!(input.down, 0);
    }
}
