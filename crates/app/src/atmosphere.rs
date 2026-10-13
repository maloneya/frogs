//! Scene-selected weather is rebuilt from presentation time; it never feeds Game.
use arpg_core::{EnvironmentLight, LinearRgb};
use arpg_gfx::{EffectVertex, OrthoCamera, PointLight};
use glam::Vec3;

const DROPS: usize = 512;
const MAX_SPLASHES: usize = 24;
const SPLASH_SEGMENTS: usize = 12;
const SPLASH_LIFE: f32 = 0.14;
const _: () = assert!(DROPS * 12 + MAX_SPLASHES * SPLASH_SEGMENTS * 6 <= arpg_gfx::MAX_EFFECT_VERTICES);

#[derive(Default)]
pub(super) struct Atmosphere {
    vertices: Vec<EffectVertex>,
}

// Two short return strokes, then darkness. Absolute phase avoids frame-rate drift.
fn flash(seconds: f64) -> f32 {
    let phase = seconds.rem_euclid(11.7);
    [(4.0, 0.09, 1.0), (4.22, 0.18, 0.65)]
        .into_iter()
        .map(|(start, length, strength)| {
            let age = phase - start;
            if (0.0..length).contains(&age) {
                strength * (1.0 - age / length) as f32
            } else {
                0.0
            }
        })
        .fold(0.0, f32::max)
}

pub(super) fn illumination(
    authored: EnvironmentLight,
    storm: bool,
    seconds: f64,
) -> EnvironmentLight {
    if !storm {
        return authored;
    }
    let flash = flash(seconds);
    let brightened = authored.with_added_intensity(flash * 0.32, flash * 1.7);
    EnvironmentLight::try_new(
        authored
            .ambient_colour()
            .lerp(LinearRgb::new(0.55, 0.68, 1.0), flash),
        brightened.ambient_intensity(),
        authored
            .directional_colour()
            .lerp(LinearRgb::new(0.68, 0.79, 1.0), flash),
        brightened.directional_intensity(),
        authored.direction(),
    )
    .expect("bounded presentation lightning")
}

impl Atmosphere {
    pub(super) fn vertices(&self) -> &[EffectVertex] {
        &self.vertices
    }

    pub(super) fn rebuild(
        &mut self,
        storm: bool,
        seconds: f64,
        camera: &OrthoCamera,
        lights: &[PointLight],
    ) {
        self.vertices.clear();
        if !storm {
            return;
        }
        let span = (camera.view_height() * 2.4).max(18.0);
        let centre = camera.target();
        let (right, _) = camera.ground_basis();
        let lightning = flash(seconds);
        let wind_speed = 1.4 + (seconds * 0.8).cos() as f32 * 0.44;
        let mut splashes = 0;
        for i in 0..DROPS {
            // Integer hashes are a spatial pattern, never ambient random input.
            let key = (i as u32)
                .wrapping_mul(747_796_405)
                .wrapping_add(2_891_336_453);
            let unit = |v: u32| (v & 0xffff) as f32 / 65535.0;
            let speed = 10.0 + unit(key.rotate_left(19)) * 4.0;
            let fall = (seconds * f64::from(speed) - f64::from(unit(key.rotate_left(5)) * 8.0)).rem_euclid(8.0) as f32;
            let location = |time: f64| {
                let wind = time * 1.4 + (time * 0.8).sin() * 0.55;
                let x = unit(key) * span - wind.rem_euclid(f64::from(span)) as f32;
                let z = unit(key.rotate_left(11)) * span + (wind * 0.4).rem_euclid(f64::from(span)) as f32;
                Vec3::new(
                    centre.x + (x - centre.x + span * 0.5).rem_euclid(span) - span * 0.5,
                    0.0,
                    centre.z + (z - centre.z + span * 0.5).rem_euclid(span) - span * 0.5,
                )
            };
            let point = location(seconds) + Vec3::Y * (8.0-fall).rem_euclid(8.0);
            let width = right * (0.006 + unit(key.rotate_left(7)) * 0.006);
            let exposure = 0.025 + unit(key.rotate_left(23)) * 0.017;
            let tail = point + Vec3::new(wind_speed, speed, -wind_speed * 0.4) * exposure;
            let colour = rain_colour(point, lightning, lights);
            // A bright centre and transparent sides remove the hard white
            // slash. Tail length follows velocity, including the current gust.
            for (a, b) in [(-1.0, 0.0), (0.0, 1.0)] {
                let head = |lane: f32| EffectVertex::new(point + width * lane,
                    colour.with_alpha(if lane == 0.0 { 0.60 } else { 0.0 }));
                let end = |lane: f32| EffectVertex::new(tail + width * lane, colour.with_alpha(0.0));
                self.vertices.extend_from_slice(&[
                    head(a), head(b), end(b), head(a), end(b), end(a),
                ]);
            }
            let age = fall / speed;
            if age < SPLASH_LIFE && splashes < MAX_SPLASHES {
                splashes += 1;
                // The ring stays where this drop hit, rather than following
                // its respawned streak or sliding with the moving camera.
                let impact = location(seconds-f64::from(age)) + Vec3::Y * 0.015;
                let t = age / SPLASH_LIFE;
                let radius = 0.025 + t * 0.11;
                let colour = rain_colour(impact, lightning, lights).with_alpha((1.0-t).powi(2) * 0.32);
                for segment in 0..SPLASH_SEGMENTS {
                    let edge = |index: usize, r: f32| {
                        let angle = index as f32 * std::f32::consts::TAU / SPLASH_SEGMENTS as f32;
                        EffectVertex::new(impact + Vec3::new(angle.cos(), 0.0, angle.sin()) * r, colour)
                    };
                    self.vertices.extend_from_slice(&[
                        edge(segment, radius), edge(segment+1, radius), edge(segment+1, radius+0.014),
                        edge(segment, radius), edge(segment+1, radius+0.014), edge(segment, radius+0.014),
                    ]);
                }
            }
        }
    }
}

fn rain_colour(point: Vec3, lightning: f32, lights: &[PointLight]) -> LinearRgb {
    let mut colour = Vec3::new(0.045, 0.065, 0.095) + Vec3::splat(lightning * 0.65);
    for light in lights {
        let d2 = point.distance_squared(light.position());
        let fade = (1.0-d2/light.range().powi(2)).max(0.0);
        colour += light.colour().to_vec3()
            * (light.intensity()*fade*fade/(1.0+d2)) * 0.12;
    }
    LinearRgb::new(colour.x, colour.y, colour.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lightning_reveals_the_scene_then_returns_to_authored_darkness() {
        let dark = EnvironmentLight::try_new(
            LinearRgb::new(0.2, 0.3, 0.6),
            0.015,
            LinearRgb::new(0.35, 0.45, 0.8),
            0.035,
            Vec3::Y,
        )
        .unwrap();
        assert_eq!(illumination(dark, false, 4.0), dark);
        assert_eq!(illumination(dark, true, 3.9), dark);
        let lit = illumination(dark, true, 4.0);
        assert!(lit.directional_intensity() > 1.5);
        assert!(illumination(dark, true, 4.22).directional_intensity() > 1.0);
        assert_eq!(illumination(dark, true, 4.5), dark);
        assert_eq!(illumination(dark, true, 11.7 + 4.0), lit);
    }
    #[test]
    fn rain_is_bounded_repeatable_and_cleared_when_the_scene_changes() {
        let camera = OrthoCamera::new(1280, 720);
        let mut rain = Atmosphere::default();
        rain.rebuild(true, 2.0, &camera, &[]);
        assert!(rain.vertices.len() >= DROPS * 12);
        assert!(rain.vertices.len() <= arpg_gfx::MAX_EFFECT_VERTICES);
        let positions: Vec<_> = rain.vertices.iter().map(|v| v.position()).collect();
        rain.rebuild(true, 2.0, &camera, &[]);
        assert_eq!(
            positions,
            rain.vertices
                .iter()
                .map(|v| v.position())
                .collect::<Vec<_>>()
        );
        rain.rebuild(true, 2.01, &camera, &[]);
        assert!(rain.vertices[0].position().y < positions[0].y);
        rain.rebuild(false, 2.01, &camera, &[]);
        assert!(rain.vertices.is_empty());
    }

    #[test]
    fn camera_motion_does_not_drag_rain_and_splashes_stay_bounded() {
        let mut camera = OrthoCamera::new(1280, 720);
        let lights = [PointLight::new(Vec3::new(-1.0, 1.4, 2.0), LinearRgb::WHITE, 35.0, 11.0)];
        let mut rain = Atmosphere::default();
        rain.rebuild(true, 2.0, &camera, &lights);
        let positions: Vec<_> = rain.vertices.iter().map(|v| v.position()).collect();
        assert!(positions.iter().any(|p| p.y == 0.015), "ground impacts accompany falling streaks");
        camera.snap_to(Vec3::new(0.2, 0.0, 0.1));
        rain.rebuild(true, 2.0, &camera, &lights);
        assert_eq!(positions.len(), rain.vertices.len());
        let fixed = positions.iter().zip(&rain.vertices).filter(|(a,b)| a.abs_diff_eq(b.position(), 1e-5)).count();
        assert!(fixed > positions.len()*9/10, "only the field's offscreen wrap boundary may move");
        for step in 0..120 {
            rain.rebuild(true, f64::from(step)/60.0, &camera, &lights);
            assert!(rain.vertices.len() <= arpg_gfx::MAX_EFFECT_VERTICES);
            assert!(rain.vertices.len().is_multiple_of(3));
            assert!(rain.vertices.iter().all(|v| v.position().is_finite()));
        }
        rain.rebuild(false, 2.0, &camera, &lights);
        assert!(rain.vertices.is_empty(), "dry scenes clear both streaks and impacts");
    }
}
