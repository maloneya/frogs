//! Tick observations become fading world geometry; nothing writes back to gameplay.

use std::collections::VecDeque;

use arpg_core::Report;
use arpg_game::Game;
use arpg_gfx::{EffectVertex, MAX_EFFECT_VERTICES};
use arpg_sim::{Alpha, AttackPhase, AttackProfile};
use glam::{Vec3, Vec4};

// Eight ticks of history plus an interpolation endpoint, bounded independently
// of the configured attack duration. Old samples remain at their observed pose.
const LIFE: f32 = 8.0;
const CAPACITY: usize = 10;
const RING_SEGMENTS: usize = 64;
const _: () = assert!(CAPACITY * 8 * 12 + RING_SEGMENTS * 18 <= MAX_EFFECT_VERTICES);

#[derive(Clone, Copy)]
struct Sample {
    tick: u64,
    centre: Vec3,
    origin: Vec3,
    radius: f32,
    profile: AttackProfile,
}

pub(crate) struct AttackEffects {
    samples: VecDeque<Sample>,
    vertices: Vec<EffectVertex>,
    observed: Option<u64>,
    active: bool,
}

impl Default for AttackEffects {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(CAPACITY),
            vertices: Vec::with_capacity(MAX_EFFECT_VERTICES),
            observed: None,
            active: false,
        }
    }
}

impl AttackEffects {
    pub(crate) fn clear(&mut self) {
        self.samples.clear();
        self.vertices.clear();
        self.observed = None;
        self.active = false;
    }

    /// Called after EACH game tick, including ticks with no intervening render.
    pub(crate) fn observe(&mut self, game: &Game) {
        let tick = game.tick();
        if self.observed == Some(tick) {
            return;
        }
        if self.observed.is_some_and(|previous| tick < previous) {
            self.clear();
        }
        self.observed = Some(tick);
        let status = game.attack_status();
        self.active = status.phase == AttackPhase::Active;
        if status.phase == AttackPhase::Startup && status.elapsed == 0 {
            self.samples.clear();
        }
        while self
            .samples
            .front()
            .is_some_and(|s| tick.saturating_sub(s.tick) > LIFE as u64 + 1)
        {
            self.samples.pop_front();
        }
        if self.active {
            let disc = game
                .attack_discs()
                .next()
                .expect("active attack has a live sample");
            if self.samples.len() == CAPACITY {
                self.samples.pop_front();
            }
            self.samples.push_back(Sample {
                tick,
                centre: disc.centre(),
                origin: game.player_pos() * Vec3::new(1.0, 0.0, 1.0),
                radius: disc.radius(),
                profile: status
                    .swing_profile
                    .expect("active attack has a committed profile"),
            });
        }
    }

    pub(crate) fn rebuild(&mut self, tick: u64, alpha: Alpha) {
        self.vertices.clear();
        let Some(last) = self.samples.back().copied() else {
            return;
        };
        // Match body interpolation: the newest segment is revealed gradually
        // between the previous and current completed tick, never extrapolated.
        let now = tick as f64 - 1.0 + f64::from(alpha.get());
        match last.profile {
            AttackProfile::Cleave => {
                for i in 0..self.samples.len() {
                    let end = self.samples[i];
                    let age = (now - end.tick as f64).max(0.0) as f32;
                    let opacity = (1.0 - age / LIFE).clamp(0.0, 1.0);
                    if opacity == 0.0 {
                        continue;
                    }
                    let start = if i == 0 {
                        // A tiny leading slash makes the first active sample visible.
                        let radial = (end.centre - end.origin).normalize_or_zero();
                        Sample {
                            centre: end.centre + Vec3::new(-radial.z, 0.0, radial.x) * 0.06,
                            ..end
                        }
                    } else {
                        self.samples[i - 1]
                    };
                    let reveal = if end.tick == tick { alpha.get() } else { 1.0 };
                    for j in 0..8 {
                        let section = |t: f32| {
                            let sample_time =
                                start.tick as f64 + (end.tick - start.tick) as f64 * f64::from(t);
                            let fade =
                                (1.0 - (now - sample_time).max(0.0) as f32 / LIFE).clamp(0.0, 1.0);
                            // Taper the beginning of the sweep and its ageing tail.
                            let taper = ((i as f32 + t) / 2.0).min(1.0) * fade;
                            let mut points = cross_section(start, end, t);
                            points[0] = points[1].lerp(points[0], taper);
                            points[2] = points[1].lerp(points[2], taper);
                            (points, fade * taper * 0.7)
                        };
                        let (a, opacity_a) = section(j as f32 / 8.0 * reveal);
                        let (b, opacity_b) = section((j + 1) as f32 / 8.0 * reveal);
                        band(
                            &mut self.vertices,
                            a,
                            b,
                            Vec3::new(0.35, 0.75, 1.0),
                            [opacity_a, opacity_b],
                        );
                    }
                }
            }
            AttackProfile::Slam => {
                let mut ring = last;
                if last.tick == tick && self.samples.len() > 1 {
                    let prev = self.samples[self.samples.len() - 2];
                    ring.centre = prev.centre.lerp(last.centre, alpha.get());
                    ring.radius = prev.radius + (last.radius - prev.radius) * alpha.get();
                }
                let age = (now - last.tick as f64).max(0.0) as f32;
                let opacity = (1.0 - age / LIFE).clamp(0.0, 1.0);
                if opacity == 0.0 {
                    return;
                }
                let colour = Vec3::new(1.0, 0.4, 0.08);
                let centre = ring.centre + Vec3::Y * 0.035;
                for i in 0..RING_SEGMENTS {
                    let section = |index: usize| {
                        let angle = index as f32 * std::f32::consts::TAU / RING_SEGMENTS as f32;
                        let radial = Vec3::new(angle.cos(), 0.0, angle.sin());
                        [
                            centre + radial * (ring.radius - 0.32).max(0.0),
                            centre + radial * (ring.radius - 0.07).max(0.0),
                            centre + radial * ring.radius,
                        ]
                    };
                    let a = section(i);
                    let b = section(i + 1);
                    band(&mut self.vertices, a, b, colour, [opacity * 0.85; 2]);
                    // Subtle interior wash distinguishes a damaging disc from a hollow ring.
                    triangle(
                        &mut self.vertices,
                        [centre, a[0], b[0]],
                        [colour.extend(opacity * 0.055); 3],
                    );
                }
            }
        }
    }

    pub(crate) fn vertices(&self) -> &[EffectVertex] {
        &self.vertices
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.int("sample_count", self.samples.len() as u64);
        out.int("vertex_count", self.vertices.len() as u64);
        out.bool("active", self.active);
        if let Some(tick) = self.observed {
            out.int("observed_tick", tick);
        }
        if let Some(sample) = self.samples.back() {
            out.text("profile", sample.profile.label());
            out.vec3("centre", sample.centre);
            out.num("radius", sample.radius);
            out.int("last_sample_tick", sample.tick);
        }
    }
}

fn cross_section(start: Sample, end: Sample, t: f32) -> [Vec3; 3] {
    let origin = start.origin.lerp(end.origin, t);
    let a = start.centre - start.origin;
    let b = end.centre - end.origin;
    let from = a.x.atan2(a.z);
    let turn = (b.x.atan2(b.z) - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let angle = from + turn * t;
    let radial = Vec3::new(angle.sin(), 0.0, angle.cos());
    let reach = a.length() + (b.length() - a.length()) * t;
    let width = start.radius + (end.radius - start.radius) * t;
    let centre = origin + radial * reach + Vec3::Y * 0.65;
    [
        centre - radial * width * 0.65,
        centre + radial * width * 0.72,
        centre + radial * width,
    ]
}

fn band(out: &mut Vec<EffectVertex>, a: [Vec3; 3], b: [Vec3; 3], colour: Vec3, opacity: [f32; 2]) {
    let transparent = colour.extend(0.0);
    let bright_a = colour.extend(opacity[0]);
    let bright_b = colour.extend(opacity[1]);
    for (i, (ca, cb)) in [
        ([transparent, bright_a], [transparent, bright_b]),
        ([bright_a, transparent], [bright_b, transparent]),
    ]
    .into_iter()
    .enumerate()
    {
        triangle(out, [a[i], b[i], a[i + 1]], [ca[0], cb[0], ca[1]]);
        triangle(out, [a[i + 1], b[i], b[i + 1]], [ca[1], cb[0], cb[1]]);
    }
}

fn triangle(out: &mut Vec<EffectVertex>, points: [Vec3; 3], colours: [Vec4; 3]) {
    assert!(
        out.len() + 3 <= MAX_EFFECT_VERTICES,
        "effect geometry exceeds its bounded stream"
    );
    out.extend(
        points
            .into_iter()
            .zip(colours)
            .map(|(p, c)| EffectVertex::new(p, c)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::{Intent, MoveDir};
    use arpg_sim::{Accumulator, Dt};

    #[test]
    fn both_effects_observe_every_active_tick_and_expire_without_changing_simulation() {
        for profile in AttackProfile::ALL {
            let mut game = Game::empty();
            game.set_attack_profile(profile);
            let mut effects = AttackEffects::default();
            let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
            let resolved = profile.resolve();
            let close = resolved.startup() + resolved.active();
            for elapsed in 0..close + 12 {
                game.step(dt, Intent::new(MoveDir::new(Vec3::X), elapsed == 0));
                // Changing the next move must not change the effect already in flight.
                if elapsed == 1 {
                    game.set_attack_profile(AttackProfile::Cleave);
                }
                let hash = game.hash();
                effects.observe(&game);
                let count = effects.samples.len();
                effects.observe(&game);
                assert_eq!(count, effects.samples.len(), "same tick cannot emit twice");
                if elapsed < resolved.startup() {
                    assert!(effects.samples.is_empty(), "no swoosh during wind-up");
                } else if elapsed < close {
                    let live = game.attack_discs().next().unwrap();
                    let sample = effects.samples.back().unwrap();
                    assert_eq!(sample.profile, profile);
                    assert_eq!(sample.centre, live.centre());
                    assert_eq!(
                        sample.origin.y, 0.0,
                        "trail uses ground pose, not body half-height"
                    );
                    assert_eq!(sample.radius, live.radius());
                    assert_eq!(sample.tick, game.tick());
                    assert_eq!(
                        effects.samples.len(),
                        (elapsed - resolved.startup() + 1) as usize
                    );
                }
                // Simulate multiple ticks between frames: observation must retain all of them.
                if elapsed % 3 == 0 || elapsed == close {
                    effects.rebuild(game.tick(), Alpha::ZERO);
                    if elapsed >= resolved.startup() && elapsed < close + 5 {
                        assert!(!effects.vertices().is_empty());
                    }
                }
                assert_eq!(
                    hash,
                    game.hash(),
                    "presentation cannot change authoritative state"
                );
            }
            effects.rebuild(game.tick(), Alpha::ZERO);
            assert!(effects.samples.is_empty());
            assert!(effects.vertices().is_empty());
        }
    }

    #[test]
    fn turn_does_not_move_old_samples_and_restart_discards_everything() {
        let mut game = Game::empty();
        let mut effects = AttackEffects::default();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        for elapsed in 0..8 {
            game.step(dt, Intent::new(MoveDir::NONE, elapsed == 0));
            effects.observe(&game);
        }
        let previous = effects.samples.front().unwrap().centre;
        game.step(dt, Intent::new(MoveDir::new(Vec3::X), false));
        effects.observe(&game);
        assert_eq!(effects.samples.front().unwrap().centre, previous);
        effects.rebuild(game.tick(), Alpha::ZERO);
        assert!(!effects.vertices().is_empty());
        effects.clear();
        assert!(effects.vertices().is_empty());
        assert!(effects.samples.is_empty());
        assert!(effects.observed.is_none());
        effects.observe(&Game::empty());
        effects.rebuild(0, Alpha::ZERO);
        assert!(effects.vertices().is_empty());
    }

    #[test]
    fn cleave_interpolation_preserves_arc_and_observed_endpoints() {
        let start = Sample {
            tick: 1,
            centre: Vec3::new(-1.8, 0.0, 1.2),
            origin: Vec3::ZERO,
            radius: 1.1,
            profile: AttackProfile::Cleave,
        };
        let end = Sample {
            tick: 2,
            centre: Vec3::new(1.8, 0.0, 1.2),
            ..start
        };
        let middle = cross_section(start, end, 0.5);
        assert!(middle[2].x.abs() < 1e-5);
        assert!((middle[2].z - (start.centre.length() + 1.1)).abs() < 1e-5);
        for (t, sample) in [(0.0, start), (1.0, end)] {
            let expected =
                sample.centre + sample.centre.normalize() * sample.radius + Vec3::Y * 0.65;
            assert!(cross_section(start, end, t)[2].distance(expected) < 1e-5);
        }
    }
}
