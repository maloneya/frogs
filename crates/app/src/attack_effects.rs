//! Tick observations become fading world geometry; nothing writes back to gameplay.

use std::collections::VecDeque;

use arpg_core::{LinearRgb, LinearRgba, Report};
use arpg_game::Game;
use arpg_gfx::{EffectVertex, MAX_EFFECT_VERTICES};
use arpg_sim::{Alpha, AttackPhase, AttackProfile};
use glam::{Vec3, Vec3Swizzles};

// Eight ticks of history plus an interpolation endpoint, bounded independently
// of the configured attack duration. Old samples remain at their observed pose.
const LIFE: f32 = 8.0;
const CAPACITY: usize = 10;
const RING_SEGMENTS: usize = 64;
const THROAT_SEGMENTS: usize = 4;
const TAIL_SEGMENTS: usize = 8;
const MAX_MARKERS: usize = 128;
const MARKER_LIFE: f32 = 6.0;
const SLAM_LIFE: u64 = 24;
const SLAM_PLUMES: usize = 10;
const DUST_SEGMENTS: usize = 8;
const SLAM_CHIPS: usize = 12;
const SLAM_RAYS: usize = 5;
const SLAM_CRACKS: usize = 9;
const SLAM_VERTICES: usize = SLAM_PLUMES * DUST_SEGMENTS * 3 + (SLAM_CHIPS + SLAM_RAYS + SLAM_CRACKS) * 6;
// Four telegraphs fit alongside player trails, hit sparks and all torch flames
// in the shared effect buffer. Gameplay admission does not size renderer memory.
pub(crate) const MAX_VISIBLE_ACTIONS: usize = 4;
pub(crate) const MAX_ATTACK_VERTICES: usize = CAPACITY * 8 * 12 + SLAM_VERTICES + MAX_MARKERS * 24 + MAX_VISIBLE_ACTIONS * RING_SEGMENTS * 9;
const _: () = assert!(MAX_ATTACK_VERTICES <= MAX_EFFECT_VERTICES);
// Three bands: blade glow, throat, body and tapered tail; each band segment is 12 vertices.
const _: () = assert!(
    (THROAT_SEGMENTS + TAIL_SEGMENTS + 2) * 3 * 12 + MAX_MARKERS * 24 <= MAX_ATTACK_VERTICES
);

/// Ground markers consume bounded, ordered observations.
pub(crate) fn visible_actions(actions: impl Iterator<Item = arpg_game::ActionStatus>) -> impl Iterator<Item = arpg_game::ActionStatus> {
    actions.filter(|action| matches!(action.kind, arpg_game::ActionKind::Swipe | arpg_game::ActionKind::Leap)
        && action.phase != AttackPhase::Idle).take(MAX_VISIBLE_ACTIONS)
}

#[derive(Clone, Copy)]
struct Sample {
    tick: u64,
    centre: Vec3,
    origin: Vec3,
    radius: f32,
    kind: SampleKind,
    end_normal: Option<Vec3>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SampleKind {
    Cleave,
    Beam,
}

impl SampleKind {
    fn label(self) -> &'static str {
        match self {
            Self::Cleave => AttackProfile::Cleave.label(),
            Self::Beam => AttackProfile::Disintegrate.label(),
        }
    }
}

/// Sampled sword attachment: both position and blade axis come from the drawn pose.
#[derive(Clone, Copy)]
pub(crate) struct BeamEmitter {
    pub tip: Vec3,
    pub direction: Vec3,
}

struct HitMarker {
    tick: u64,
    centre: Vec3,
    side: Vec3,
}

#[derive(Clone, Copy)]
struct SlamImpact {
    tick: u64,
    centre: Vec3,
    forward: Vec3,
}

pub(crate) struct AttackEffects {
    slam: Option<SlamImpact>,
    markers: VecDeque<HitMarker>,
    samples: VecDeque<Sample>,
    vertices: Vec<EffectVertex>,
    observed: Option<u64>,
    active: bool,
}

impl Default for AttackEffects {
    fn default() -> Self {
        Self {
            slam: None,
            markers: VecDeque::with_capacity(MAX_MARKERS),
            samples: VecDeque::with_capacity(CAPACITY),
            vertices: Vec::with_capacity(MAX_EFFECT_VERTICES),
            observed: None,
            active: false,
        }
    }
}

impl AttackEffects {
    pub(crate) fn contact(&mut self, tick: u64, centre: Vec3, side: Vec3) {
        if self.markers.len() == MAX_MARKERS { self.markers.pop_front(); }
        self.markers.push_back(HitMarker { tick, centre, side });
    }

    pub(crate) fn clear(&mut self) {
        self.slam = None;
        self.markers.clear();
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
        if self.slam.is_some_and(|impact| tick.saturating_sub(impact.tick) >= SLAM_LIFE) {
            self.slam = None;
        }
        while self.markers.front().is_some_and(|m| tick.saturating_sub(m.tick) > MARKER_LIFE as u64) {
            self.markers.pop_front();
        }

        let beam = game.beam();
        if let Some(beam) = beam {
            let direction = (beam.end() - beam.start()).normalize_or_zero();
            for point in game.beam_hits() {
                self.contact(tick, Vec3::new(point.x, 1.1, point.y),
                    Vec3::new(direction.y, 0.0, -direction.x));
            }
        }
        let status = game.attack_status();
        if !status.channeling {
            let facing = game.player_presentation(Alpha::ONE).facing();
            let side = Vec3::new(facing.cos(), 0.0, -facing.sin());
            for defeat in game.defeats() {
                self.contact(tick, Vec3::new(defeat.position.x, 0.9, defeat.position.y), side);
            }
        }
        self.active = status.phase == AttackPhase::Active;
        if self.active && status.swing_profile == Some(AttackProfile::Slam) {
            let resolved = status.swing_resolved.expect("active slam has committed timing");
            // The blade reaches the floor at the end of contact. Earlier damage
            // samples still run; this single visual burst cannot drive gameplay.
            if status.elapsed == resolved.startup() + resolved.active() - 1 {
                let facing = game.player_presentation(Alpha::ONE).facing();
                let forward = Vec3::new(facing.sin(), 0.0, facing.cos());
                self.slam = Some(SlamImpact {
                    tick,
                    centre: game.player_pos() * Vec3::new(1.0, 0.0, 1.0) + forward * 1.6,
                    forward,
                });
            }
        }
        if let Some(beam) = beam {
            // A beam uses the current endpoint; only Cleave needs trail history.
            self.samples.clear();
            self.samples.push_back(Sample {
                tick,
                centre: Vec3::new(beam.end().x, 0.0, beam.end().y),
                origin: Vec3::new(beam.start().x, 0.0, beam.start().y),
                radius: arpg_sim::Beam::RADIUS,
                kind: SampleKind::Beam,
                end_normal: beam.end_normal().map(|n| Vec3::new(n.x, 0.0, n.y)),
            });
            return;
        }
        if self.samples.back().is_some_and(|s| s.kind == SampleKind::Beam) { self.samples.clear(); }
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
        if self.active && status.swing_profile == Some(AttackProfile::Cleave) {
            let disc = game
                .attack_discs()
                .next()
                .expect("active attack has a live sample");
            let origin = game.player_pos() * Vec3::new(1.0, 0.0, 1.0);
            let (sin, cos) = game.player_presentation(Alpha::ONE).facing().sin_cos();
            let lateral = Vec3::new(cos, 0.0, -sin);
            // The right-handed blade sweeps opposite the gameplay disc order.
            // Reflect only the visual offset in the committed facing plane;
            // retain each observed world origin so later movement cannot drag it.
            let centre = origin + (disc.centre() - origin).reflect(lateral);
            if self.samples.len() == CAPACITY {
                self.samples.pop_front();
            }
            self.samples.push_back(Sample {
                tick,
                centre,
                origin,
                radius: disc.radius(),
                end_normal: None,
                kind: SampleKind::Cleave,
            });
        }
    }

    pub(crate) fn rebuild(&mut self, game: &Game, alpha: Alpha, beam_emitter: Option<BeamEmitter>) {

        let tick = game.tick();
        self.vertices.clear();
        for attack in visible_actions(game.action_statuses()) { action_telegraph(&mut self.vertices, attack, alpha); }
        let now = tick as f64 - 1.0 + f64::from(alpha.get());
        if let Some(impact) = self.slam {
            slam_geometry(&mut self.vertices, impact, (now - impact.tick as f64) as f32);
        }
        for marker in &self.markers {
            let age = tick.saturating_sub(marker.tick) as f32 + alpha.get();
            let opacity = (1.0 - age / MARKER_LIFE).clamp(0.0, 1.0);
            if opacity == 0.0 { continue; }
            let colour = LinearRgba::new(1.0, 0.95, 0.7, opacity);
            // Four bright diamond rays sit above each actual contact, including
            // lethal hits whose body has already left the simulation.
            for (axis, width) in [(marker.side, Vec3::Y), (-marker.side, Vec3::Y),
                (Vec3::Y, marker.side), (-Vec3::Y, marker.side)] {
                let inner = marker.centre + axis * 0.08;
                let middle = marker.centre + axis * 0.22;
                let outer = marker.centre + axis * (0.42 + age * 0.02);
                for tip in [inner, outer] {
                    triangle(&mut self.vertices,
                        [middle - width * 0.055, tip, middle + width * 0.055], [colour; 3]);
                }
            }
        }
        let Some(last) = self.samples.back().copied() else {
            return;
        };
        // Match body interpolation: the newest segment is revealed gradually
        // between the previous and current completed tick, never extrapolated.
        match last.kind {
            SampleKind::Beam => {
                if let Some(emitter) = beam_emitter {
                    beam_geometry(&mut self.vertices, game, last, emitter, now);
                }
            }
            SampleKind::Cleave => {
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
                            centre: end.centre + Vec3::new(radial.z, 0.0, -radial.x) * 0.06,
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
                            LinearRgb::new(0.35, 0.75, 1.0),
                            [opacity_a, opacity_b],
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn vertices(&self) -> &[EffectVertex] {
        &self.vertices
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.int("hit_markers", self.markers.len() as u64);
        out.int("sample_count", self.samples.len() as u64);
        out.int("vertex_count", self.vertices.len() as u64);
        out.bool("active", self.active);
        out.bool("slam_impact", self.slam.is_some());
        if let Some(impact) = self.slam {
            out.vec3("slam_centre", impact.centre);
            out.int("slam_tick", impact.tick);
        }
        if let Some(tick) = self.observed {
            out.int("observed_tick", tick);
        }
        if let Some(sample) = self.samples.back() {
            out.text("profile", sample.kind.label());
            out.vec3("centre", sample.centre);
            out.num("radius", sample.radius);
            out.int("last_sample_tick", sample.tick);
        }
    }
}

fn slam_geometry(out: &mut Vec<EffectVertex>, impact: SlamImpact, age: f32) {
    if !(0.0..SLAM_LIFE as f32).contains(&age) { return; }
    let progress = age / SLAM_LIFE as f32;
    let fade = (1.0 - progress).powi(2);
    let side = Vec3::new(impact.forward.z, 0.0, -impact.forward.x);
    let radial = |i: usize, count: usize| {
        let angle = i as f32 * std::f32::consts::TAU / count as f32;
        side * angle.cos() + impact.forward * angle.sin()
    };
    // Broken, unequal ground splinters mark the impact without tracing reach.
    for i in 0..SLAM_CRACKS {
        let axis = radial(i, SLAM_CRACKS);
        let across = Vec3::new(-axis.z, 0.0, axis.x);
        let length = 0.55 + (i * 5 % 7) as f32 * 0.11;
        let start = impact.centre + axis * 0.12 + Vec3::Y * 0.018;
        let bend = start + axis * length * 0.55 + across * (0.04 * (i % 3) as f32 - 0.04);
        let end = start + axis * length;
        let colour = LinearRgb::new(0.035, 0.026, 0.018).with_alpha(fade * 0.75);
        triangle(out, [start - across * 0.025, bend, start + across * 0.025], [colour; 3]);
        triangle(out, [bend - across * 0.014, end, bend + across * 0.014], [colour; 3]);
    }
    let eye = arpg_core::isometric_eye_direction();
    let right = Vec3::new(eye.z, 0.0, -eye.x).normalize();
    let up = eye.cross(right).normalize();
    for i in 0..SLAM_PLUMES {
        let axis = radial(i, SLAM_PLUMES);
        let reach = 0.65 + (i * 3 % 5) as f32 * 0.17;
        let centre = impact.centre + axis * (0.15 + reach * progress.sqrt())
            + Vec3::Y * (0.10 + progress * (0.25 + (i % 3) as f32 * 0.12));
        let width = 0.16 + progress * (0.40 + (i % 4) as f32 * 0.07);
        let colour = LinearRgb::new(0.34, 0.29, 0.23);
        let opacity = fade * 0.48;
        for j in 0..DUST_SEGMENTS {
            let edge = |n: usize| {
                let angle = n as f32 * std::f32::consts::TAU / DUST_SEGMENTS as f32;
                let mut point = centre + right * angle.cos() * width + up * angle.sin() * width * 0.65;
                point.y = point.y.max(0.025);
                point
            };
            triangle(out, [centre, edge(j), edge(j + 1)],
                [colour.with_alpha(opacity), colour.with_alpha(0.0), colour.with_alpha(0.0)]);
        }
    }
    // Chips follow short baked ballistic arcs; there are no simulated particles.
    for i in 0..SLAM_CHIPS {
        let axis = radial(i, SLAM_CHIPS);
        let flight = (0.42 + (i * 7 % 5) as f32 * 0.10).min(1.0);
        let t = (progress / flight).min(1.0);
        let centre = impact.centre + axis * (0.15 + t * (0.7 + (i % 4) as f32 * 0.25))
            + Vec3::Y * (0.035 + 4.0 * t * (1.0 - t) * (0.22 + (i % 3) as f32 * 0.16));
        let across = Vec3::new(-axis.z, 0.0, axis.x) * (0.035 + (i % 3) as f32 * 0.01);
        let colour = LinearRgb::new(0.105, 0.078, 0.05).with_alpha(fade * 0.85);
        let a = centre - across; let b = centre + across;
        let top = centre + axis * 0.06 + Vec3::Y * 0.05;
        triangle(out, [a, top, b], [colour; 3]);
        triangle(out, [a, b, centre - axis * 0.045], [colour; 3]);
    }
    if age < 3.0 {
        let flash = (1.0 - age / 3.0).powi(2);
        for i in 0..SLAM_RAYS {
            let axis = (radial(i, SLAM_RAYS) + Vec3::Y * (0.4 + (i % 3) as f32 * 0.3)).normalize();
            let across = right * 0.018;
            let start = impact.centre + axis * 0.06 + Vec3::Y * 0.055;
            let middle = start + axis * 0.14;
            let end = start + axis * (0.28 + (i % 3) as f32 * 0.12 + age * 0.08);
            let bright = LinearRgb::new(0.9, 0.88, 0.80).with_alpha(flash * 0.85);
            let clear = LinearRgba::new(0.9, 0.88, 0.80, 0.0);
            triangle(out, [middle - across, start, middle + across], [bright, clear, bright]);
            triangle(out, [middle - across, end, middle + across], [bright, clear, bright]);
        }
    }
}



// The centreline has a short curved throat followed by a straight beam and
// tapered end. Query every generated segment, since the interpolated sword can
// cross a wall that the authoritative player-centred damage ray misses.
fn beam_geometry(
    out: &mut Vec<EffectVertex>,
    game: &Game,
    sample: Sample,
    emitter: BeamEmitter,
    now: f64,
) {
    const SECTIONS: usize = THROAT_SEGMENTS + TAIL_SEGMENTS + 2;
    let origin = emitter.tip;
    let end = sample.centre + Vec3::Y * origin.y;
    if (end - origin).dot(sample.centre - sample.origin) <= 0.0 {
        return;
    }
    let cast = |a: Vec3, b: Vec3| {
        game.solid_hit(a.xz(), b.xz())
            .expect("finite sampled geometry")
    };
    // An animated sword can protrude through nearby solid geometry. Do not
    // start a visual beam on the far side of that obstacle.
    if cast(sample.origin, origin).is_some() {
        return;
    }
    let direction = (end - origin).normalize_or_zero();
    let side = Vec3::new(direction.z, 0.0, -direction.x);
    let length = origin.distance(end);
    let emergence_end = origin + direction * length.min(1.0) * 0.5;
    let taper_start = end - direction * (length * 0.5).min(1.8);
    let control = origin + emitter.direction * origin.distance(emergence_end) * 0.5;
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    // Position, width fraction, opacity fraction; fixed storage is reused on the stack.
    let mut sections = [(Vec3::ZERO, 0.0, 0.0); SECTIONS];
    for (i, section) in sections[..=THROAT_SEGMENTS].iter_mut().enumerate() {
        let t = i as f32 / THROAT_SEGMENTS as f32;
        *section = (
            origin
                .lerp(control, t)
                .lerp(control.lerp(emergence_end, t), t),
            0.12 + 0.88 * smooth(t),
            1.0,
        );
    }
    for (i, section) in sections[THROAT_SEGMENTS + 1..].iter_mut().enumerate() {
        let t = i as f32 / TAIL_SEGMENTS as f32;
        let fade = 1.0 - smooth(t);
        *section = (taper_start.lerp(end, t), fade, fade);
    }
    let mut count = SECTIONS;
    let mut cap = sample.end_normal.map(|normal| (end, normal));
    for i in 0..SECTIONS - 1 {
        let a = sections[i];
        let b = sections[i + 1];
        if let Some(hit) = cast(a.0, b.0) {
            if hit.normal() == glam::Vec2::ZERO {
                return;
            }
            let point = a.0.lerp(b.0, hit.fraction());
            sections[i + 1] = (point, a.1 + (b.1 - a.1) * hit.fraction(), 0.0);
            cap = Some((point, Vec3::new(hit.normal().x, 0.0, hit.normal().y)));
            count = i + 2;
            break;
        }
    }
    let trim = |point: Vec3| {
        cap.map_or(point, |(end, normal)| {
            point + normal * (end - point).dot(normal).max(0.0)
        })
    };
    let blade_side = Vec3::Y.cross(emitter.direction).normalize_or_zero();
    let blade_start = origin - emitter.direction * 0.55;
    let blade_clear = cast(blade_start, origin).is_none();
    let glow = 1.0 + 0.12 * (now as f32 * 0.7).sin();
    for (width, colour, opacity) in [
        (sample.radius * 1.7, LinearRgb::new(1.0, 0.12, 0.025), 0.4),
        (sample.radius, LinearRgb::new(1.0, 0.45, 0.06), 0.85),
        (sample.radius * 0.22, LinearRgb::new(1.0, 0.9, 0.55), 1.0),
    ] {
        let section = |point: Vec3, axis: Vec3, radius: f32| {
            [point - axis * radius, point, point + axis * radius].map(trim)
        };
        if blade_clear {
            band(
                out,
                section(blade_start, blade_side, width * 0.08),
                section(origin, blade_side, width * 0.18 * glow),
                colour,
                [0.0, opacity],
            );
        }
        for pair in sections[..count].windows(2) {
            let [a, b] = [pair[0], pair[1]];
            band(
                out,
                section(a.0, side, width * a.1),
                section(b.0, side, width * b.1),
                colour,
                [opacity * a.2, opacity * b.2],
            );
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

fn band(out: &mut Vec<EffectVertex>, a: [Vec3; 3], b: [Vec3; 3], colour: LinearRgb, opacity: [f32; 2]) {
    let transparent = colour.with_alpha(0.0);
    let bright_a = colour.with_alpha(opacity[0]);
    let bright_b = colour.with_alpha(opacity[1]);
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

// The disc is the gameplay-owned strike shape. Filling it communicates time to
// impact without expanding the promised reach; a leap briefly flashes on landing.
fn action_telegraph(out: &mut Vec<EffectVertex>, attack: arpg_game::ActionStatus, alpha: Alpha) {
    if !matches!(attack.kind, arpg_game::ActionKind::Swipe | arpg_game::ActionKind::Leap) { return; }
    let leap = attack.kind == arpg_game::ActionKind::Leap;
    let landing = leap && attack.phase == AttackPhase::Recovery && attack.elapsed < 9;
    if !matches!(attack.phase, AttackPhase::Startup | AttackPhase::Active) && !landing { return; }
    let progress = ((attack.elapsed as f32 + alpha.get()) / attack.duration as f32).clamp(0.0, 1.0);
    let active = if leap { landing } else { attack.phase == AttackPhase::Active };
    let colour = if active { LinearRgb::new(1.0, 0.035, 0.012) }
        else if leap { LinearRgb::new(0.65, 0.12, 1.0) }
        else { LinearRgb::new(1.0, 0.45, 0.035) };
    let centre = Vec3::new(attack.centre.x, 0.045, attack.centre.y);
    for i in 0..RING_SEGMENTS {
        let radial = |j: usize| {
            let angle = j as f32 * std::f32::consts::TAU / RING_SEGMENTS as f32;
            Vec3::new(angle.cos(), 0.0, angle.sin())
        };
        let a = radial(i);
        let b = radial(i + 1);
        let edge = colour.with_alpha(if active { 0.95 } else { 0.7 });
        let inner = attack.radius - 0.045;
        triangle(out, [centre + a * inner, centre + a * attack.radius, centre + b * attack.radius], [edge; 3]);
        triangle(out, [centre + a * inner, centre + b * attack.radius, centre + b * inner], [edge; 3]);
        let fill = attack.radius * if active || (leap && attack.phase == AttackPhase::Active) { 1.0 } else { progress };
        triangle(out, [centre, centre + a * fill, centre + b * fill],
            [colour.with_alpha(if active { 0.45 * (1.0 - progress) } else { 0.18 }); 3]);
    }
}

fn triangle(out: &mut Vec<EffectVertex>, points: [Vec3; 3], colours: [LinearRgba; 3]) {
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
    fn melee_effects_observe_consumed_samples_and_expire_without_changing_simulation() {
        for profile in AttackProfile::ALL {
            let mut game = Game::empty();
            game.set_attack_profile(profile);
            let mut effects = AttackEffects::default();
            let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
            let Some(resolved) = profile.resolve(arpg_sim::Tuning::default()) else { continue };
            let close = resolved.startup() + resolved.active();
            for elapsed in 0..close + SLAM_LIFE as u32 + 2 {
                game.step(dt, Intent::new(MoveDir::new(Vec3::X), arpg_core::AimDir::new(Vec3::X).unwrap(), elapsed == 0));
                // Changing the next move must not change the effect already in flight.
                if elapsed == 1 {
                    game.set_attack_profile(AttackProfile::Cleave);
                }
                let hash = game.hash();
                effects.observe(&game);
                let count = effects.samples.len();
                effects.observe(&game);
                assert_eq!(count, effects.samples.len(), "same tick cannot emit twice");
                if profile == AttackProfile::Slam {
                    assert!(effects.samples.is_empty(), "Slam has no trail history");
                } else if elapsed < resolved.startup() {
                    assert!(effects.samples.is_empty(), "no swoosh during wind-up");
                } else if elapsed < close {
                    let live = game.attack_discs().next().unwrap();
                    let sample = effects.samples.back().unwrap();
                    assert!(sample.kind == SampleKind::Cleave);
                    // This test swings toward +X: mirror the lateral Z offset,
                    // while preserving gameplay reach and forward placement.
                    let offset = live.centre() - sample.origin;
                    let expected = sample.origin + Vec3::new(offset.x, offset.y, -offset.z);
                    assert!(sample.centre.abs_diff_eq(expected, 1e-5));
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
                    effects.rebuild(
                        &game, Alpha::ZERO, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
                    let begins = if profile == AttackProfile::Slam { close - 1 } else { resolved.startup() };
                    if elapsed >= begins && elapsed < close + 5 {
                        assert!(!effects.vertices().is_empty());
                    }
                }
                assert_eq!(
                    hash,
                    game.hash(),
                    "presentation cannot change authoritative state"
                );
            }
            effects.rebuild(
                &game, Alpha::ZERO, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
            assert!(effects.samples.is_empty());
            assert!(effects.vertices().is_empty());
        }
    }

    #[test]
    fn slam_impact_waits_for_the_floor_strike_stays_in_world_space_and_expires() {
        let mut game = Game::empty();
        game.set_attack_profile(AttackProfile::Slam);
        let mut effects = AttackEffects::default();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut captured = None;
        let mut interpolation = Accumulator::default();
        assert_eq!(interpolation.pending(Dt::SECS * 0.5).count(), 0);
        for elapsed in 0..40 {
            game.step(dt, Intent::new(MoveDir::new(Vec3::X), arpg_core::AimDir::FORWARD, elapsed == 0));
            // Selection cannot rewrite the committed Slam or refresh its burst.
            if elapsed == 1 { game.set_attack_profile(AttackProfile::Cleave); }
            let hash = game.hash();
            effects.observe(&game);
            effects.observe(&game);
            assert!(effects.samples.is_empty(), "Slam must never record an unused disc trail");
            let mut report = Report::default();
            effects.report(&mut report);
            let report: serde_json::Value = serde_json::from_str(&report.finish()).unwrap();
            assert_eq!(report["sample_count"], 0);
            assert!(report.get("profile").is_none());
            if elapsed < 13 {
                assert!(effects.slam.is_none());
                effects.rebuild(&game, Alpha::ONE, None);
                assert!(effects.vertices().is_empty(), "no reach ring or dust before the floor strike");
            } else if elapsed < 37 {
                let impact = effects.slam.unwrap();
                if elapsed == 13 {
                    assert_eq!(impact.tick, game.tick());
                    assert!(impact.centre.abs_diff_eq(game.player_pos() * Vec3::new(1.0, 0.0, 1.0) + Vec3::Z * 1.6, 1e-5));
                    captured = Some((impact.tick, impact.centre));
                }
                assert_eq!(Some((impact.tick, impact.centre)), captured);
                for alpha in [Alpha::ZERO, interpolation.alpha(), Alpha::ONE] {
                    effects.rebuild(&game, alpha, None);
                    assert!(effects.vertices().len() <= SLAM_VERTICES);
                    assert!(effects.vertices().iter().all(|v| v.position().is_finite()
                        && v.position().y >= 0.0 && v.position().xz().distance(impact.centre.xz()) < 2.8));
                }
                if elapsed > 13 { assert!(!effects.vertices().is_empty()); }
            } else {
                assert!(effects.slam.is_none());
                effects.rebuild(&game, Alpha::ONE, None);
                assert!(effects.vertices().is_empty());
            }
            assert_eq!(game.hash(), hash);
        }
        // A restart during the burst must also discard its independent lifetime.
        game.set_attack_profile(AttackProfile::Slam);
        for elapsed in 0..14 {
            game.step(dt, Intent::new(MoveDir::NONE, arpg_core::AimDir::FORWARD, elapsed == 0));
            effects.observe(&game);
        }
        assert!(effects.slam.is_some());
        effects.clear();
        assert!(effects.slam.is_none());
        effects.observe(&Game::empty());
        effects.rebuild(&Game::empty(), Alpha::ONE, None);
        assert!(effects.vertices().is_empty());
    }

    #[test]
    fn turn_does_not_move_old_samples_and_restart_discards_everything() {
        let mut game = Game::empty();
        let mut effects = AttackEffects::default();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        for elapsed in 0..8 {
            game.step(dt, Intent::new(MoveDir::NONE, arpg_core::AimDir::FORWARD, elapsed == 0));
            effects.observe(&game);
        }
        let previous = effects.samples.front().unwrap().centre;
        game.step(dt, Intent::new(MoveDir::new(Vec3::X), arpg_core::AimDir::new(Vec3::X).unwrap(), false));
        effects.observe(&game);
        assert_eq!(effects.samples.front().unwrap().centre, previous);
        effects.rebuild(
            &game, Alpha::ZERO, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
        assert!(!effects.vertices().is_empty());
        effects.clear();
        assert!(effects.vertices().is_empty());
        assert!(effects.samples.is_empty());
        assert!(effects.observed.is_none());
        effects.observe(&Game::empty());
        effects.rebuild(&Game::empty(), Alpha::ZERO, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
        assert!(effects.vertices().is_empty());
    }

    #[test]
    fn cleave_interpolation_preserves_arc_and_observed_endpoints() {
        let start = Sample {
            tick: 1,
            centre: Vec3::new(1.8, 0.0, 1.2),
            origin: Vec3::ZERO,
            radius: 1.1,
            kind: SampleKind::Cleave,
            end_normal: None,
        };
        let end = Sample {
            tick: 2,
            centre: Vec3::new(-1.8, 0.0, 1.2),
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

#[cfg(test)]
mod beam_tests {
    use super::*;
    use arpg_core::{AimDir, Intent, MoveDir};
    use arpg_sim::{Accumulator, Dt};

    #[test]
    fn beam_bands_and_glow_stop_at_flat_and_diagonal_thin_walls_after_turning() {
        for yaw in [0.0, std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4] {
            let wall = arpg_sim::Wall::new(glam::Vec2::new(0.0, 2.0), glam::Vec2::new(6.0, 0.1), yaw).unwrap();
            let mut game = Game::from_scene(&arpg_game::Scene { walls: vec![wall], ..Default::default() }.into()).unwrap();
            game.set_attack_profile(AttackProfile::Disintegrate);
            let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
            let mut effects = AttackEffects::default();
            for aim in [AimDir::new(Vec3::X).unwrap(), AimDir::FORWARD] {
                game.step(dt, Intent::new(MoveDir::NONE, aim, false).with_attack_held(true));
                effects.observe(&game);
            }
            let end = Vec3::new(0.0, 0.0, 2.0 - 0.05 / yaw.cos());
            assert!(effects.samples.back().unwrap().centre.abs_diff_eq(end, 0.00001));
            for alpha in [Alpha::ZERO, Alpha::ONE] {
                effects.rebuild(
                    &game, alpha, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
                assert!(!effects.vertices().is_empty() && effects.vertices().len()<= 504);
                assert!(effects.vertices().iter().any(|v| (v.position().z - end.z).abs() < 0.00001));
                for vertex in effects.vertices() {
                    let point = vertex.position();
                    let local_z = point.x * yaw.sin() + (point.z - 2.0) * yaw.cos();
                    assert!(local_z <= -0.05 + 0.00001, "beam glow crossed wall: yaw={yaw}, vertex={point}");
                }
            }
        }
    }

    #[test]
    fn beam_geometry_stays_bounded_tracks_aim_and_clears_on_release() {
        let mut game = Game::empty();
        game.set_attack_profile(AttackProfile::Disintegrate);
        let mut effects = AttackEffects::default();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        for tick in 0..200 {
            let aim = if tick < 100 { AimDir::FORWARD } else { AimDir::new(Vec3::X).unwrap() };
            game.step(dt, Intent::new(MoveDir::NONE, aim, false).with_attack_held(true));
            let hash = game.hash();
            effects.observe(&game);
            effects.observe(&game);
            effects.rebuild(
                &game, Alpha::ONE, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
            assert_eq!(game.hash(), hash);
            assert_eq!(effects.samples.len(), 1, "beam keeps only its current endpoint");
            assert_eq!(effects.vertices().len(), 504);
            let end = effects.samples.back().unwrap().centre;
            assert!(end.abs_diff_eq(aim.as_vec3() * arpg_sim::Beam::LENGTH, 0.00001));
        }
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
        effects.observe(&game);
        effects.rebuild(
            &game, Alpha::ZERO, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
        assert!(effects.vertices().is_empty());
        assert!(effects.samples.is_empty());
    }
    #[test]
    fn beam_markers_include_lethal_hits_expire_and_reset() {
        let mut game = Game::empty();
        game.set_attack_profile(AttackProfile::Disintegrate);
        let target = game.place(glam::Vec2::new(0.0, 3.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
        let mut effects = AttackEffects::default();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        for tick in 0..=24 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true));
            effects.observe(&game);
            effects.observe(&game);
            if tick % 12 == 0 {
                assert_eq!(effects.markers.len(), 1);
                effects.rebuild(
                    &game, Alpha::ONE, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
                assert_eq!(effects.vertices().len(), 504 + 24);
            }
        }
        assert!(game.health(target).is_none());
        assert_eq!(effects.markers.len(), 1, "killing pulse leaves a marker");
        for _ in 0..8 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
            effects.observe(&game);
        }
        effects.rebuild(
            &game, Alpha::ONE, Some(BeamEmitter { tip: Vec3::Y * 0.85, direction: Vec3::Z }));
        assert!(effects.markers.is_empty());
        assert!(effects.vertices().is_empty());
        effects.clear();
        assert!(effects.markers.is_empty());
    }

}

#[cfg(test)]
mod enemy_tests {
    use super::*;
    use arpg_core::{AimDir, Intent, MoveDir};
    use arpg_game::{GameScene};
    use arpg_sim::{Accumulator, Dt};
use arpg_game::Scene;
use arpg_game::Placed;

    #[test]
    fn telegraph_matches_committed_disc_is_bounded_and_disappears_in_recovery() {
        let mut game = Game::from_scene(&GameScene {
            engine: Scene { bodies: vec![Placed { pos: (0.0, 1.4), what: arpg_game::ActorDefinition::attacker() }], ..Default::default() },
            ..Default::default()
        }).unwrap();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut effects = AttackEffects::default();
        for tick in 0..84 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
            let hash = game.hash();
            effects.observe(&game);
            effects.rebuild(&game, Alpha::ONE, None);
            let attack = game.action_statuses().next().unwrap();
            if tick < 45 {
                assert_eq!(effects.vertices.len(), RING_SEGMENTS * 9);
                for vertex in &effects.vertices {
                    assert!(vertex.position().xz().distance(attack.centre) <= attack.radius + 0.00001);
                }
            } else { assert!(effects.vertices.is_empty()); }
            assert_eq!(game.hash(), hash);
        }
        let body = game.action_statuses().next().unwrap().body;
        game.despawn_body(body);
        effects.rebuild(&game, Alpha::ONE, None);
        assert!(effects.vertices.is_empty());
    }
}

#[cfg(test)]
mod leap_tests {
    use super::*;
    use arpg_core::{AimDir, Intent, MoveDir};
    use arpg_game::GameScene;
    use arpg_sim::{Accumulator, Dt};
use arpg_game::Scene;
use arpg_game::Placed;

    #[test]
    fn simultaneous_leap_markers_stay_bounded_and_retire_after_landing() {
        let positions = [(0.0,4.0),(4.0,0.0),(0.0,-4.0),(-4.0,0.0),(3.0,3.0),(-3.0,-3.0)];
        // Waiting actors hold their authored leap positions; pursuit is a separate behavior.
        let actor = arpg_game::ActorDefinition {
            actions: vec![arpg_game::ActionDefinition::Leap(arpg_game::LeapAction::default())],
            behavior: arpg_game::BehaviorNode::Action(0),
            ..arpg_game::ActorDefinition::passive_hostile()
        };
        let mut game = Game::from_scene(&GameScene {
            engine: Scene { bodies: positions.into_iter().map(|pos| Placed { pos, what: actor.clone() }).collect(), ..Default::default() },
            ..Default::default()
        }).unwrap();
        let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
        let mut effects = AttackEffects::default();
        for tick in 0..88 {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
            let hash = game.hash();
            effects.observe(&game);
            for alpha in [Alpha::ZERO, Alpha::ONE] {
                effects.rebuild(&game, alpha, None);
                assert!(effects.vertices.len() <= MAX_VISIBLE_ACTIONS * RING_SEGMENTS * 9);
                if tick == 36 { assert_eq!(effects.vertices.len(), 4 * RING_SEGMENTS * 9); }
                if tick == 87 { assert_eq!(effects.vertices.len(), 3 * RING_SEGMENTS * 9); }
                assert!(effects.vertices.iter().all(|v| v.position().is_finite()));
                assert_eq!(game.hash(), hash);
            }
        }
        game.start_scene(&GameScene::default()).unwrap();
        effects.rebuild(&game, Alpha::ONE, None);
        assert!(effects.vertices.is_empty());
    }
}

#[cfg(test)]
mod observation_budget_tests {
    use super::*;
    #[test]
    fn routine_actions_do_not_consume_telegraph_capacity_and_extra_markers_are_bounded() {
        let mut game = Game::empty();
        let id = game.place(glam::Vec2::new(0.0, 4.0), arpg_game::ActorDefinition::attacker()).unwrap();
        let mut leap = game.action_status(id).unwrap();
        // Observation budgeting remains valid independently of combat policy.
        leap.kind = arpg_game::ActionKind::Leap;
        leap.phase = AttackPhase::Startup;
        leap.duration = 42;
        leap.radius = 0.9;
        let mut movement = leap;
        movement.kind = arpg_game::ActionKind::Move;
        movement.phase = AttackPhase::Active;
        let observations = std::iter::repeat_n(movement, 1000)
            .chain(std::iter::repeat_n(leap, MAX_VISIBLE_ACTIONS + 10));
        let mut vertices = Vec::new();
        for action in visible_actions(observations) { action_telegraph(&mut vertices, action, Alpha::ONE); }
        assert_eq!(vertices.len(), MAX_VISIBLE_ACTIONS * RING_SEGMENTS * 9);
        assert!(vertices.len() <= MAX_ATTACK_VERTICES);
    }
}
