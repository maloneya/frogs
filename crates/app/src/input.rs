//! The only place a physical key is translated into an [`Action`].
//!
//! This is the layer `arpg-core` deliberately does not have and `arpg-sim`
//! deliberately cannot reach: winit is named here and nowhere downstream.

use winit::{event::MouseButton, keyboard::KeyCode};

use crate::ui::{Menu, MenuKey};
use arpg_core::{Action, ActionMask, Actions, InputState};
use arpg_sim::AttackProfile;

/// Fixed attacks, in deterministic input priority order. HUD and input use
/// this same table; there is no mutable loadout or menu selection to reset.
pub(crate) const ATTACK_BINDINGS: [(Action, AttackProfile, &str); 3] = [
    (Action::Attack, AttackProfile::Cleave, "1/LMB"),
    (Action::AttackSecondary, AttackProfile::Slam, "2/RMB"),
    (Action::AttackTertiary, AttackProfile::Disintegrate, "3/Space"),
];

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
    ("w", KeyCode::KeyW, Some(Action::MoveUp), Some(MenuKey::Previous)),
    ("s", KeyCode::KeyS, Some(Action::MoveDown), Some(MenuKey::Next)),
    ("a", KeyCode::KeyA, Some(Action::MoveLeft), None),
    ("d", KeyCode::KeyD, Some(Action::MoveRight), None),
    ("up", KeyCode::ArrowUp, Some(Action::MoveUp), Some(MenuKey::Previous)),
    ("down", KeyCode::ArrowDown, Some(Action::MoveDown), Some(MenuKey::Next)),
    ("left", KeyCode::ArrowLeft, Some(Action::MoveLeft), None),
    ("right", KeyCode::ArrowRight, Some(Action::MoveRight), None),
    ("1", KeyCode::Digit1, Some(Action::Attack), None),
    ("2", KeyCode::Digit2, Some(Action::AttackSecondary), None),
    ("3", KeyCode::Digit3, Some(Action::AttackTertiary), None),
    ("space", KeyCode::Space, Some(Action::AttackTertiary), None),
    ("e", KeyCode::KeyE, Some(Action::Interact), None),
    ("f2", KeyCode::F2, None, Some(MenuKey::Scenes)),
    ("enter", KeyCode::Enter, None, Some(MenuKey::Accept)),
    ("escape", KeyCode::Escape, None, Some(MenuKey::Close)),
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
    mouse_down: [bool; 2],
    mouse_accepted: [bool; 2],
    aim: CursorAim,
}

impl Controls {
    /// Returns whether the event was handled, including modal unbound keys and
    /// duplicate presses. Only unhandled events may reach app debug shortcuts.
    pub(crate) fn on_key(
        &mut self,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> bool {
        let was_open = self.menu.open();
        let binding = BINDINGS.iter().enumerate().find(|(_, (_, bound, ..))| *bound == key);
        let menu_key = binding.and_then(|(_, (_, _, _, menu))| *menu);
        let consumed = was_open || matches!(menu_key, Some(MenuKey::Scenes));
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
            self.menu.on_key(menu_key);
        }
        if was_open != self.menu.open() {
            self.clear_gameplay();
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

    /// Cancel accepted input and pending edges together, without forgetting
    /// physical buttons that still need a release before their next press.
    fn clear_gameplay(&mut self) {
        self.accepted = 0;
        self.mouse_accepted = [false; 2];
        self.state = InputState::default();
    }

    /// Restart discards gameplay and the scene picker, but a physically held key
    /// must still be released before it can act in the new playtest.
    pub(crate) fn restart(&mut self) {
        self.clear_gameplay();
        self.menu = Menu::default();
        self.aim = CursorAim::default();
    }

    /// Focus loss discards held keys and unsampled edges, but preserves the menu.
    pub(crate) fn release_all(&mut self) {
        self.down = 0;
        self.mouse_down = [false; 2];
        self.aim.cursor = CursorPosition::Retained;
        self.clear_gameplay();
    }

    /// Physical mouse buttons share the action latch with keyboard attacks.
    pub(crate) fn on_mouse(&mut self, button: MouseButton, pressed: bool) {
        let index = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            _ => return,
        };
        if self.mouse_down[index] == pressed { return; }
        self.mouse_down[index] = pressed;
        self.mouse_accepted[index] = pressed && !self.menu.open();
        self.sync();
    }

    /// Cursor pixels from either the window or the harness.
    pub(crate) fn on_cursor(&mut self, pixel: glam::Vec2) -> Result<(), &'static str> {
        if !pixel.is_finite() { return Err("cursor pixels must be finite"); }
        self.aim.cursor = CursorPosition::Pixel(pixel);
        Ok(())
    }

    /// Losing the pointer retains a valid direction, rather than inventing one.
    pub(crate) fn cursor_left(&mut self) { self.aim.cursor = CursorPosition::Retained; }

    pub(crate) fn aim(&mut self, camera: &arpg_gfx::OrthoCamera, viewport: (u32, u32), player: glam::Vec3) -> arpg_core::AimDir {
        if !self.menu.open() { self.aim.update(camera, viewport, player); }
        self.aim.direction
    }

    /// Sample once per simulation tick; zero-tick frames must retain edges.
    pub(crate) fn sample(&mut self) -> Actions {
        self.state.sample()
    }

    /// Resolve a single attack per tick. Fresh presses beat held buttons;
    /// ties use slot order, independent of native event delivery order.
    /// Only the winning button supplies the held level, so an unrelated held
    /// swing button cannot sustain a released beam.
    pub(crate) fn attack_input(&self, actions: Actions) -> (Option<AttackProfile>, bool, bool) {
        let binding = ATTACK_BINDINGS.iter()
            .find(|(action, ..)| actions.just_pressed(*action))
            .or_else(|| ATTACK_BINDINGS.iter().find(|(action, ..)| actions.held(*action)));
        binding.map_or((None, false, false), |&(action, profile, _)| {
            (Some(profile), actions.just_pressed(action), actions.held(action))
        })
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
        for (accepted, action) in self.mouse_accepted.into_iter().zip([Action::Attack, Action::AttackSecondary]) {
            if accepted { held.insert(action); }
        }
        self.state.set_held(held);
    }

    pub(crate) fn menu(&self) -> &Menu {
        &self.menu
    }

    pub(crate) fn menu_mut(&mut self) -> &mut Menu {
        &mut self.menu
    }

}

/// Absence of a screen coordinate is a device state; the aim is always valid.
#[derive(Default)]
enum CursorPosition {
    #[default]
    Retained,
    Pixel(glam::Vec2),
}

struct CursorAim {
    cursor: CursorPosition,
    direction: arpg_core::AimDir,
}

impl Default for CursorAim {
    fn default() -> Self {
        Self { cursor: CursorPosition::Retained, direction: arpg_core::AimDir::FORWARD }
    }
}

impl CursorAim {
    fn update(&mut self, camera: &arpg_gfx::OrthoCamera, viewport: (u32, u32), player: glam::Vec3) {
        let CursorPosition::Pixel(pixel) = self.cursor else { return; };
        if pixel.x < 0.0 || pixel.y < 0.0
            || pixel.x >= viewport.0 as f32 || pixel.y >= viewport.1 as f32 {
            return;
        }
        let Ok(point) = camera.ground_point(pixel, viewport.0, viewport.1) else { return; };
        let direction = point - player;
        // Two centimetres avoids a direction flip from subpixel projection noise.
        if direction.length_squared() > 0.02 * 0.02
            && let Ok(aim) = arpg_core::AimDir::new(direction) {
            self.direction = aim;
        }
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
        controls.on_key(key, true, false);
        controls.on_key(key, false, false);
    }

    fn step_controls(controls: &mut Controls, game: &mut arpg_game::Game) {
        let actions = controls.sample();
        let (profile, pressed, held) = controls.attack_input(actions);
        if let Some(profile) = profile
            && game.attack_status().profile != profile {
            game.set_attack_profile(profile);
        }
        let dt = arpg_sim::Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
        game.step(dt, arpg_core::Intent::new(arpg_core::MoveDir::NONE,
            arpg_core::AimDir::FORWARD, pressed).with_attack_held(held));
    }

    #[test]
    fn bound_attacks_drive_committed_swings_and_release_channels_through_game() {
        // Predicted before stepping: Cleave owns ticks 0..21 even if Slam is
        // pressed at 1; a fresh Slam at 22 owns 22..51. Beam starts at 52,
        // stops on release at 53, and a new primary press starts Cleave.
        let run = || {
            let mut controls = Controls::default();
            let mut game = arpg_game::Game::empty();
            let mut hashes = Vec::new();
            for tick in 0..55 {
                match tick {
                    0 | 54 => {
                        controls.on_mouse(MouseButton::Left, true);
                        controls.on_mouse(MouseButton::Left, false);
                    },
                    1 | 22 => {
                        controls.on_mouse(MouseButton::Right, true);
                        controls.on_mouse(MouseButton::Right, false);
                    },
                    52 => { controls.on_key(KeyCode::Space, true, false); }
                    53 => { controls.on_key(KeyCode::Space, false, false); }
                    _ => {}
                }
                step_controls(&mut controls, &mut game);
                let status = game.attack_status();
                match tick {
                    0..=21 => assert_eq!(status.swing_profile, Some(AttackProfile::Cleave)),
                    22..=51 => assert_eq!(status.swing_profile, Some(AttackProfile::Slam)),
                    52 => assert!(status.channeling),
                    53 => { assert!(!status.channeling); assert_eq!(status.phase, arpg_sim::AttackPhase::Idle); }
                    54 => assert_eq!(status.swing_profile, Some(AttackProfile::Cleave)),
                    _ => unreachable!(),
                }
                hashes.push(game.hash());
            }
            hashes
        };
        assert_eq!(run(), run(), "identical button inputs replay tick for tick");
    }

    #[test]
    fn new_presses_beat_holds_and_simultaneous_presses_use_slot_order() {
        let mut controls = Controls::default();
        controls.on_key(KeyCode::Digit3, true, false);
        let _ = controls.sample();
        tap(&mut controls, "2");
        let actions = controls.sample();
        assert_eq!(controls.attack_input(actions), (Some(AttackProfile::Slam), true, false));
        tap(&mut controls, "2");
        tap(&mut controls, "1");
        let actions = controls.sample();
        assert_eq!(controls.attack_input(actions), (Some(AttackProfile::Cleave), true, false));
        controls.on_key(KeyCode::Digit3, false, false);
        controls.on_key(KeyCode::Space, true, false);
        let actions = controls.sample();
        assert_eq!(controls.attack_input(actions), (Some(AttackProfile::Disintegrate), true, true));
    }

    #[test]
    fn picker_navigation_is_modal_and_does_not_leak_keys() {
        let mut controls = Controls::default();
        controls.on_key(KeyCode::ArrowUp, true, false);
        tap(&mut controls, "f2");
        assert_eq!(controls.sample().move_axis(), glam::Vec2::ZERO);
        assert!(matches!(
            controls.menu_mut().take_request(),
            Some(crate::ui::MenuRequest::RefreshScenes)
        ));
        for _ in 0..10 {
            tap(&mut controls, "down");
        }
        assert_eq!(controls.menu().picker().selected(), 1, "selection clamps at the last row");
        tap(&mut controls, "right");
        tap(&mut controls, "escape");
        assert!(!controls.menu().open());
        controls.on_key(KeyCode::ArrowUp, true, true);
        assert_eq!(controls.sample().move_axis(), glam::Vec2::ZERO);
        controls.on_key(KeyCode::ArrowUp, false, false);
        controls.on_key(KeyCode::ArrowUp, true, false);
        assert!(controls.sample().held(Action::MoveUp));
        tap(&mut controls, "f2");
        tap(&mut controls, "f2");
        assert!(!controls.menu().open());
        assert!(controls.menu_mut().take_request().is_none());
    }

    #[test]
    fn modal_transitions_discard_edges_and_require_fresh_gameplay_presses() {
        let mut controls = Controls::default();
        controls.on_key(KeyCode::Space, true, false);
        controls.on_key(KeyCode::KeyW, true, false);
        // Open before any tick has consumed the attack edge.
        tap(&mut controls, "f2");
        assert!(controls.menu().open());
        let actions = controls.sample();
        assert!(!actions.just_pressed(Action::AttackTertiary));
        assert_eq!(actions.move_axis(), glam::Vec2::ZERO);
        tap(&mut controls, "right");
        for key in ["space", "1", "2", "3"] {
            tap(&mut controls, key);
        }
        let actions = controls.sample();
        assert_eq!(controls.attack_input(actions), (None, false, false));
        assert_eq!(controls.held().move_axis(), glam::Vec2::ZERO);

        let esc = key_named("escape").unwrap();
        assert!(controls.on_key(esc, true, false), "closing must consume Escape");
        assert!(!controls.menu().open());
        assert!(controls.on_key(esc, true, false), "a duplicate must not reach quit");
        // A held W cannot start moving through repeat OR duplicate injection.
        controls.on_key(KeyCode::KeyW, true, true);
        controls.on_key(KeyCode::KeyW, true, false);
        assert_eq!(controls.sample().move_axis(), glam::Vec2::ZERO);
        controls.on_key(KeyCode::KeyW, false, false);
        controls.on_key(KeyCode::KeyW, true, false);
        assert!(controls.sample().held(Action::MoveUp));
        tap(&mut controls, "space");
        assert!(controls.sample().just_pressed(Action::AttackTertiary));
        assert!(!controls.sample().just_pressed(Action::AttackTertiary));
    }

    #[test]
    fn focus_loss_discards_an_unsampled_attack_and_held_movement() {
        let mut controls = Controls::default();
        tap(&mut controls, "space");
        controls.on_key(KeyCode::KeyD, true, false);
        controls.release_all();
        let actions = controls.sample();
        assert!(!actions.just_pressed(Action::AttackTertiary));
        assert_eq!(actions.move_axis(), glam::Vec2::ZERO);
    }

    /// The bug the key-level bitset exists to prevent.
    #[test]
    fn releasing_one_of_two_keys_bound_to_the_same_action_keeps_it_held() {
        let mut input = Controls::default();
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
        let mut input = Controls::default();
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
        let mut input = Controls::default();
        input.on_key(KeyCode::KeyW, true, false);
        input.on_key(KeyCode::KeyD, true, false);
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
        assert_eq!(key_named("f1"), None);
        let mut controls = Controls::default();
        assert!(!controls.on_key(KeyCode::F1, true, false));
        assert!(!controls.menu().open());
        assert_eq!(key_named(""), None);
    }

    /// Unbound keys must fall through untouched, so the debug keys keep working.
    #[test]
    fn an_unbound_key_changes_nothing() {
        let mut input = Controls::default();
        input.on_key(KeyCode::KeyV, true, false);
        assert_eq!(input.down, 0);
    }
    /// E follows the same edge latch and modal capture as attacks. Two nearby
    /// objects make a repeated edge a gameplay bug, even for one-shot objects.
    #[test]
    fn interaction_is_one_edge_and_is_captured_by_menus() {
        let mut controls = Controls::default();
        controls.on_key(KeyCode::KeyE, true, false);
        assert!(controls.sample().just_pressed(Action::Interact));
        assert!(!controls.sample().just_pressed(Action::Interact));
        controls.on_key(KeyCode::KeyE, true, true);
        assert!(!controls.sample().just_pressed(Action::Interact));
        tap(&mut controls, "f2");
        tap(&mut controls, "e");
        assert!(!controls.sample().just_pressed(Action::Interact));
        tap(&mut controls, "escape");
        assert!(!controls.sample().just_pressed(Action::Interact));
        tap(&mut controls, "e");
        assert!(controls.sample().just_pressed(Action::Interact));
    }

}

#[cfg(test)]
mod aiming_tests {
    use super::*;
    use glam::{Vec2, Vec3};

    #[test]
    fn cursor_projection_tracks_camera_and_retains_aim_at_the_player() {
        let mut camera = arpg_gfx::OrthoCamera::new(800, 600);
        camera.snap_to(Vec3::new(5.0, 0.0, -3.0));
        camera.zoom_by(0.5);
        let mut controls = Controls::default();
        let player = Vec3::new(5.0, 0.0, -3.0);
        let target = player + Vec3::X * 4.0;
        let ndc = camera.view_proj().project_point3(target);
        let pixel = Vec2::new((ndc.x + 1.0) * 400.0, (1.0 - ndc.y) * 300.0);
        controls.on_cursor(pixel).unwrap();
        let aimed = controls.aim(&camera, (800, 600), player);
        assert!(aimed.as_vec3().abs_diff_eq(Vec3::X, 1e-4));
        controls.on_cursor(Vec2::new(400.0, 300.0)).unwrap();
        assert_eq!(controls.aim(&camera, (800, 600), player), aimed);
        assert!(controls.on_cursor(Vec2::splat(f32::NAN)).is_err());
        assert_eq!(controls.aim(&camera, (0, 0), player), aimed);
        for outside in [Vec2::new(-1.0, 300.0), Vec2::new(400.0, -1.0),
            Vec2::new(800.0, 300.0), Vec2::new(400.0, 600.0)] {
            controls.on_cursor(outside).unwrap();
            assert_eq!(controls.aim(&camera, (800, 600), player), aimed);
        }
        controls.release_all();
        assert_eq!(controls.aim(&camera, (800, 600), player), aimed);
        controls.restart();
        assert_eq!(controls.aim(&camera, (800, 600), player), arpg_core::AimDir::FORWARD);
    }

    #[test]
    fn menu_captures_mouse_and_aim_and_keyboard_release_does_not_cancel_mouse() {
        let camera = arpg_gfx::OrthoCamera::new(800, 600);
        let mut controls = Controls::default();
        controls.on_mouse(MouseButton::Left, true);
        assert!(controls.sample().just_pressed(Action::Attack));
        controls.on_key(KeyCode::Digit1, true, false);
        controls.on_key(KeyCode::Digit1, false, false);
        assert!(controls.held().contains(Action::Attack));
        controls.on_key(KeyCode::F2, true, false);
        controls.on_cursor(Vec2::new(750.0, 300.0)).unwrap();
        assert_eq!(controls.aim(&camera, (800, 600), Vec3::ZERO), arpg_core::AimDir::FORWARD);
        assert!(!controls.sample().just_pressed(Action::Attack));
        controls.on_key(KeyCode::Escape, true, false);
        assert!(!controls.held().contains(Action::Attack));
        controls.on_mouse(MouseButton::Left, false);
        controls.on_mouse(MouseButton::Left, true);
        assert!(controls.sample().just_pressed(Action::Attack));
        controls.release_all();
        assert!(!controls.sample().just_pressed(Action::Attack));
    }
}
