//! Damage observations reveal temporary bars; this state never enters simulation.

use std::collections::HashMap;

use arpg_game::Game;
use arpg_gfx::{OrthoCamera, Quad, QuadSink};
use arpg_sim::{Alpha, EntityId, ENEMY_HIT_POINTS, TICK_HZ};
use glam::{Vec2, Vec3, Vec4};

const HOLD_SECONDS: f32 = 2.0;
const FADE_SECONDS: f32 = 0.5;
const WIDTH: f32 = 32.0;
const HEIGHT: f32 = 5.0;
/// Above the ordinary enemy's head, in world metres.
const ANCHOR_HEIGHT: f32 = 1.35;
/// Leave room for menus and readouts in the shared quad buffer.
const MAX_VISIBLE: usize = 1024;
const _: () = assert!(MAX_VISIBLE * 2 + 2048 <= arpg_gfx::MAX_QUADS);

struct Remembered {
    health: u8,
    damaged_at: u64,
}

#[derive(Default)]
pub(crate) struct EnemyHealth {
    // Only damaged live enemies need history. Drawing follows the world's
    // ordered iterator, never hash iteration. Expired entries remember health
    // so an old wound cannot reveal the bar again every frame.
    damaged: HashMap<EntityId, Remembered>,
}

impl EnemyHealth {
    pub(crate) fn clear(&mut self) {
        self.damaged.clear();
    }

    /// Observe authoritative health once per drawn frame, including offscreen
    /// enemies. Multiple ticks between frames collapse to the latest health.
    /// Removal/eviction retires history; App clears it when a run replaces IDs.
    pub(crate) fn draw(
        &mut self,
        game: &Game,
        alpha: Alpha,
        camera: &OrthoCamera,
        viewport: Vec2,
        sink: &mut QuadSink<'_>,
    ) {
        self.damaged.retain(|id, _| game.health(*id).is_some_and(|hp| hp < ENEMY_HIT_POINTS));
        let view = camera.view_proj();
        let mut visible = 0;
        for enemy in game.enemy_presentations(alpha) {
            let hp = game.health(enemy.id()).expect("enemy presentation has health");
            if hp == ENEMY_HIT_POINTS { continue; }
            let remembered = self.damaged.entry(enemy.id()).or_insert(Remembered {
                health: hp, damaged_at: game.tick(),
            });
            if hp < remembered.health {
                remembered.damaged_at = game.tick();
            }
            remembered.health = hp;
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
                Vec4::new(0.008, 0.006, 0.006, opacity * 0.9)));
            sink.push(Quad::solid(Vec4::new(x + 1.0, y + 1.0,
                (WIDTH - 2.0) * f32::from(hp) / f32::from(ENEMY_HIT_POINTS), HEIGHT - 2.0),
                Vec4::new(0.65, 0.055, 0.025, opacity)));
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
    use arpg_sim::{Accumulator, Dt, Template};

    #[test]
    fn damage_reveals_proportional_bars_then_expires_and_removal_retires_history() {
        let mut game = Game::empty();
        let id = game.place(Vec2::new(-1.8, 1.2), Template::BODY).unwrap();
        let mut bars = EnemyHealth::default();
        let mut buffer = QuadBuffer::default();
        let camera = OrthoCamera::new(1280, 720);
        let viewport = Vec2::new(1280.0, 720.0);
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert!(buffer.as_slice().is_empty(), "full health is invisible");
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        // Cleave starts at this enemy and deals exactly one of its three hits.
        for tick in 0..12 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, tick == 0));
        }
        assert_eq!(game.health(id), Some(ENEMY_HIT_POINTS - 1));
        let hash = game.hash();
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert_eq!(game.hash(), hash, "presentation cannot change the simulation");
        assert_eq!(buffer.as_slice().len(), 2);
        assert_eq!(buffer.as_slice()[1].rect().z, 20.0);
        let first = buffer.as_slice()[0].rect();
        for _ in 0..30 { game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)); }
        bars.draw(&game, Alpha::ONE, &camera, viewport, &mut buffer.sink());
        assert_ne!(buffer.as_slice()[0].rect(), first, "bar follows knockback");
        for _ in 0..120 { game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)); }
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert!(buffer.as_slice().is_empty(), "expired damage stays hidden");
        bars.draw(&game, Alpha::ONE, &camera, viewport, &mut buffer.sink());
        assert!(buffer.as_slice().is_empty());
        assert!(game.despawn_body(id));
        let replacement = game.place(Vec2::new(-1.8, 1.2), Template::BODY).unwrap();
        assert_ne!(id, replacement);
        bars.draw(&game, Alpha::ZERO, &camera, viewport, &mut buffer.sink());
        assert!(bars.damaged.is_empty());
        assert!(buffer.as_slice().is_empty());
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
        let id = game.place(Vec2::new(0.0, 95.7), Template::BODY).unwrap();
        game.set_attack_profile(arpg_sim::AttackProfile::Slam);
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut bars = EnemyHealth::default();
        let mut buffer = QuadBuffer::default();
        let mut camera = OrthoCamera::new(1280, 720);
        // As in the three-hit scenario, the arena wall keeps the enemy in reach.
        // The first wound expires; the second must reveal one-third health.
        for tick in 0..860 {
            let movement = if tick < 630 { MoveDir::new(Vec3::Z) } else { MoveDir::NONE };
            game.step(dt, Intent::new(movement, AimDir::FORWARD, matches!(tick, 630 | 810 | 840)));
            camera.snap_to(game.player_pos());
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
