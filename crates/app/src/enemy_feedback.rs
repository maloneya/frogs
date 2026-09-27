//! Shared damage observations drive contact flashes and temporary health bars.
//! This state never enters simulation.

use arpg_core::LinearRgba;
use std::collections::HashMap;

use arpg_game::Game;
use arpg_gfx::{OrthoCamera, Quad, QuadSink};
use arpg_sim::{Alpha, EntityId, TICK_HZ};
use glam::{Vec2, Vec3, Vec4};

const FLASH_SECONDS: f32 = 0.1;
const HOLD_SECONDS: f32 = 2.0;
const FADE_SECONDS: f32 = 0.5;
const WIDTH: f32 = 32.0;
const HEIGHT: f32 = 5.0;
/// Above the ordinary enemy's head, in world metres.
const ANCHOR_HEIGHT: f32 = 1.35;
/// Leave room for gameplay HUD in the shared quad buffer.
const MAX_VISIBLE: usize = 1024;
const _: () = assert!(MAX_VISIBLE * 2 + 2048 <= arpg_gfx::MAX_QUADS);

struct Remembered {
    health: u8,
    damaged_at: u64,
}

#[derive(Default)]
pub(crate) struct EnemyFeedback {
    // Only damaged live enemies need history. Drawing follows the world's
    // ordered iterator, never hash iteration. Expired entries remember health
    // so an old wound cannot reveal the bar again every frame.
    damaged: HashMap<EntityId, Remembered>,
}

impl EnemyFeedback {
    pub(crate) fn clear(&mut self) {
        self.damaged.clear();
    }

    /// Observe once before preparing a drawn frame, including offscreen enemies.
    /// Catch-up ticks collapse to the latest health; first visible feedback starts
    /// here, so a skipped redraw cannot consume the entire flash. Repeated
    /// observations of unchanged health never refresh it. Removal/eviction retires
    /// history; App clears it when a run replaces IDs.
    pub(crate) fn observe(&mut self, game: &Game) {
        self.damaged.retain(|id, _| game.health(*id).zip(game.max_health(*id)).is_some_and(|(hp, max)| hp < max));
        for enemy in game.character_presentations(Alpha::ONE) {
            let Some((hp, max)) = game.health(enemy.id()).zip(game.max_health(enemy.id())) else { continue; };
            if hp == max { continue; }
            let remembered = self.damaged.entry(enemy.id()).or_insert(Remembered {
                health: hp, damaged_at: game.tick(),
            });
            if hp < remembered.health {
                remembered.damaged_at = game.tick();
            }
            remembered.health = hp;
        }
    }

    pub(crate) fn flash(&self, id: EntityId, tick: u64, alpha: Alpha) -> f32 {
        self.damaged.get(&id).map_or(0.0, |remembered| {
            let age = (tick.saturating_sub(remembered.damaged_at) as f32 + alpha.get())
                / TICK_HZ as f32;
            (1.0 - age / FLASH_SECONDS).clamp(0.0, 1.0)
        })
    }

    pub(crate) fn draw(
        &self,
        game: &Game,
        alpha: Alpha,
        camera: &OrthoCamera,
        viewport: Vec2,
        sink: &mut QuadSink<'_>,
    ) {
        let view = camera.view_proj();
        let mut visible = 0;
        for enemy in game.character_presentations(alpha) {
            let Some(remembered) = self.damaged.get(&enemy.id()) else { continue; };
            let hp = remembered.health;
            let age = (game.tick().saturating_sub(remembered.damaged_at) as f32 + alpha.get())
                / TICK_HZ as f32;
            let opacity = opacity(age);
            if opacity <= 0.0 || visible == MAX_VISIBLE { continue; }
            let ndc = view.project_point3(enemy.ground_position() + Vec3::Y * ANCHOR_HEIGHT);
            if !ndc.is_finite() || !(0.0..=1.0).contains(&ndc.z) { continue; }
            let x = (ndc.x + 1.0) * 0.5 * viewport.x - WIDTH * 0.5;
            let y = (1.0 - ndc.y) * 0.5 * viewport.y - HEIGHT;
            // Do not pin offscreen enemies' bars to the viewport edge.
            if x < 0.0 || y < 0.0 || x + WIDTH > viewport.x || y + HEIGHT > viewport.y {
                continue;
            }
            sink.push(Quad::solid(Vec4::new(x, y, WIDTH, HEIGHT),
                LinearRgba::new(0.008, 0.006, 0.006, opacity * 0.9)));
            sink.push(Quad::solid(Vec4::new(x + 1.0, y + 1.0,
                (WIDTH - 2.0) * f32::from(hp) / f32::from(game.max_health(enemy.id()).expect("remembered live health")), HEIGHT - 2.0),
                LinearRgba::new(0.65, 0.055, 0.025, opacity)));
            visible += 1;
        }
    }
}

fn opacity(age: f32) -> f32 {
    ((HOLD_SECONDS + FADE_SECONDS - age) / FADE_SECONDS).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::{AimDir, Intent, MoveDir};
    use arpg_gfx::QuadBuffer;
    use arpg_sim::{Accumulator, Dt};

    #[test]
    fn confirmed_hits_flash_then_fade_without_refreshing_on_duplicate_frames() {
        let mut game = Game::empty();
        let target = game.place(Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        let missed = game.place(Vec2::new(10.0, 10.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        let mut feedback = EnemyFeedback::default();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        // Cleave has six startup ticks: only step seven may reveal contact.
        for step in 1..=13 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, step == 1));
            let hash = game.hash();
            feedback.observe(&game);
            feedback.observe(&game);
            let expected = if step < 7 { 0.0 } else { (13 - step) as f32 / 6.0 };
            assert!((feedback.flash(target, game.tick(), Alpha::ZERO) - expected).abs() < 1e-6);
            assert_eq!(feedback.flash(missed, game.tick(), Alpha::ZERO), 0.0);
            if step == 7 {
                assert_eq!(game.health(target), Some(1));
                assert!((feedback.flash(target, game.tick(), Alpha::ONE) - 5.0 / 6.0).abs() < 1e-6);
            }
            assert_eq!(game.hash(), hash);
        }
        assert!(game.despawn_body(target));
        let replacement = game.place(Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        feedback.observe(&game);
        assert_eq!(feedback.flash(target, game.tick(), Alpha::ZERO), 0.0);
        assert_eq!(feedback.flash(replacement, game.tick(), Alpha::ZERO), 0.0);
    }

    #[test]
    fn beam_pulses_refresh_contact_and_defeat_retires_it() {
        let mut game = Game::empty();
        let id = game.place(Vec2::new(0.0, 4.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        game.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut feedback = EnemyFeedback::default();
        // Beam pulses on ticks 0, 12 and 24; there is no push to move the target.
        for tick in 0..=24 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true));
            feedback.observe(&game);
            match tick {
                0 | 12 => assert_eq!(feedback.flash(id, game.tick(), Alpha::ZERO), 1.0),
                6 | 18 | 24 => assert_eq!(feedback.flash(id, game.tick(), Alpha::ZERO), 0.0),
                _ => {}
            }
        }
        assert!(!game.is_alive(id));
        assert!(feedback.damaged.is_empty());
    }

    #[test]
    fn damage_reveals_proportional_bars_then_expires_and_removal_retires_history() {
        let mut game = Game::empty();
        let id = game.place(Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        let mut bars = EnemyFeedback::default();
        let mut buffer = QuadBuffer::default();
        let camera = OrthoCamera::new(1280, 720);
        let viewport = Vec2::new(1280.0, 720.0);
        bars.observe(&game);
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert!(buffer.as_slice().is_empty(), "full health is invisible");
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        // Cleave starts at this enemy and removes two of its three health points.
        for tick in 0..12 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, tick == 0));
        }
        assert_eq!(game.health(id), Some(1));
        let hash = game.hash();
        bars.observe(&game);
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert_eq!(game.hash(), hash, "presentation cannot change the simulation");
        assert_eq!(buffer.as_slice().len(), 2);
        assert_eq!(buffer.as_slice()[1].rect().z, 10.0);
        let first = buffer.as_slice()[0].rect();
        for _ in 0..30 { game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)); }
        bars.observe(&game);
        bars.draw(&game, Alpha::ONE, &camera, viewport, &mut buffer.sink());
        assert_ne!(buffer.as_slice()[0].rect(), first, "bar follows knockback");
        for _ in 0..120 { game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)); }
        bars.observe(&game);
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert!(buffer.as_slice().is_empty(), "expired damage stays hidden");
        bars.observe(&game);
        bars.draw(&game, Alpha::ONE, &camera, viewport, &mut buffer.sink());
        assert!(buffer.as_slice().is_empty());
        assert!(game.despawn_body(id));
        let replacement = game.place(Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        assert_ne!(id, replacement);
        bars.observe(&game);
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert!(bars.damaged.is_empty());
        assert!(buffer.as_slice().is_empty());
    }

    #[test]
    fn immune_civilians_render_without_health_feedback() {
        let mut game = Game::empty();
        let civilian = game.place(Vec2::new(4.0, 4.0), arpg_game::ActorDefinition::civilian()).unwrap();
        assert!(game.character_presentations(Alpha::ONE).any(|actor| actor.id() == civilian));
        assert_eq!(game.enemy_count(), 0);
        assert_eq!(game.max_health(civilian), None);
        let hash = game.hash();
        let mut feedback = EnemyFeedback::default();
        feedback.observe(&game);
        feedback.observe(&game);
        assert!(feedback.damaged.is_empty());
        assert_eq!(game.hash(), hash);
    }

    #[test]
    fn fade_is_elapsed_time_based() {
        assert_eq!(opacity(0.0), 1.0);
        assert_eq!(opacity(2.0), 1.0);
        assert_eq!(opacity(2.25), 0.5);
        assert_eq!(opacity(2.5), 0.0);
        assert_eq!(opacity(100.0), 0.0);
    }

    #[test]
    fn another_hit_reveals_an_expired_bar_and_defeat_removes_it() {
        let mut game = Game::empty();
        let id = game.place(Vec2::new(0.0, 95.7), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        game.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut bars = EnemyFeedback::default();
        let mut buffer = QuadBuffer::default();
        let mut camera = OrthoCamera::new(1280, 720);
        // Separate one-tick beam holds deal one point each at the arena wall.
        // The first wound expires; the second must reveal one-third health.
        for tick in 0..860 {
            let movement = if tick < 630 { MoveDir::new(Vec3::Z) } else { MoveDir::NONE };
            game.step(dt, Intent::new(movement, AimDir::FORWARD, false)
                .with_attack_held(matches!(tick, 630 | 810 | 840)));
            camera.snap_to(game.player_pos());
            bars.observe(&game);
            bars.draw(&game, Alpha::ZERO, &camera, Vec2::new(1280.0, 720.0), &mut buffer.sink());
            match tick {
                650 => {
                    assert_eq!(game.health(id), Some(2));
                    assert_eq!(buffer.as_slice()[1].rect().z, 20.0);
                }
                800 => assert!(buffer.as_slice().is_empty()),
                830 => {
                    assert_eq!(game.health(id), Some(1));
                    assert_eq!(buffer.as_slice()[1].rect().z, 10.0);
                }
                859 => {
                    assert!(!game.is_alive(id));
                    assert!(buffer.as_slice().is_empty());
                    assert!(bars.damaged.is_empty());
                }
                _ => {}
            }
        }
    }
}
