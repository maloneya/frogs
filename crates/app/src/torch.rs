//! Presentation geometry rebuilt from the game's scene-owned torches.
use arpg_core::{Instance, LinearRgb, MAX_POINT_LIGHTS, PointLight};
use arpg_game::TorchSpec;
use arpg_gfx::EffectVertex;
use glam::Vec3;

const FLAME_TONGUES: usize = 3;
const FLAME_SEGMENTS: usize = 2;
const EMBERS: usize = 4;
const FLAME_VERTICES: usize = FLAME_TONGUES * FLAME_SEGMENTS * 12 + EMBERS * 3;
const _: () = assert!(
    FLAME_VERTICES * MAX_POINT_LIGHTS + crate::attack_effects::MAX_ATTACK_VERTICES
        <= arpg_gfx::MAX_EFFECT_VERTICES
);

pub(super) struct Torches {
    lights: Vec<PointLight>,
    stands: Vec<Instance>,
    effects: Vec<EffectVertex>,
}

impl Default for Torches {
    fn default() -> Self {
        Self {
            lights: Vec::with_capacity(MAX_POINT_LIGHTS),
            stands: Vec::with_capacity(MAX_POINT_LIGHTS),
            effects: Vec::with_capacity(arpg_gfx::MAX_EFFECT_VERTICES),
        }
    }
}

impl Torches {
    pub(super) fn lights(&self) -> &[PointLight] {
        &self.lights
    }
    pub(super) fn stand(&self) -> &[Instance] {
        &self.stands
    }
    pub(super) fn effects(&self) -> &[EffectVertex] {
        &self.effects
    }

    pub(super) fn rebuild(
        &mut self,
        torches: impl Iterator<Item = TorchSpec>,
        attacks: &[EffectVertex],
        seconds: f64,
    ) {
        self.lights.clear();
        self.stands.clear();
        self.effects.clear();
        for torch in torches {
            assert!(self.lights.len() < MAX_POINT_LIGHTS, "scene light capacity");
            let authored = torch.light();
            let phase = seconds + f64::from(authored.position().x * 0.37 + authored.position().z * 0.19);
            let flicker = 1.0 + (seconds * 17.0).sin() as f32 * 0.07 + (seconds * 29.0).sin() as f32 * 0.035;
            let light = authored.with_scaled_intensity(flicker);
            self.lights.push(light);
            let ground = Vec3::new(light.position().x, 0.0, light.position().z);
            self.stands.push(Instance::new(ground, Vec3::ONE, LinearRgb::WHITE));
            self.flames(ground, light.colour(), phase);
        }
        assert_eq!(self.effects.len(), self.lights.len() * FLAME_VERTICES);
        self.effects.extend_from_slice(attacks);
    }

    fn flames(&mut self, ground: Vec3, colour: LinearRgb, seconds: f64) {
        // Transparent edges and a bright centre replace the solid diamond.
        // All motion is reconstructed from presentation time, with no emitter
        // state to survive a restart or scene eviction.
        let right = Vec3::new(0.707_106_77, 0.0, -0.707_106_77);
        for tongue in 0..FLAME_TONGUES {
            let phase = seconds * (6.0 + tongue as f64) + tongue as f64 * 2.1;
            let height = 1.0 + 0.13 * tongue as f32 + phase.sin() as f32 * 0.10;
            let origin = ground + Vec3::Y * 0.50 + right * ((tongue as f32 - 1.0) * 0.16);
            let row = |index: usize, lane: f32| {
                let t = index as f32 / FLAME_SEGMENTS as f32;
                let bend = (phase + f64::from(t) * 2.5).sin() as f32 * 0.16 * t;
                let width = (1.0-t) * (0.30 - tongue as f32 * 0.045) + 0.006;
                let position = origin + Vec3::Y * (t * height) + right * (bend + lane * width);
                let hot = colour.lerp(LinearRgb::new(3.0, 2.2, 0.65), (1.0-t) * 0.8);
                let alpha = if lane == 0.0 { (1.0-t) * 0.9 } else { 0.0 };
                EffectVertex::new(position, hot.with_alpha(alpha))
            };
            for segment in 0..FLAME_SEGMENTS {
                for (a, b) in [(-1.0, 0.0), (0.0, 1.0)] {
                    self.effects.extend_from_slice(&[
                        row(segment, a), row(segment, b), row(segment+1, b),
                        row(segment, a), row(segment+1, b), row(segment+1, a),
                    ]);
                }
            }
        }
        for ember in 0..EMBERS {
            let age = (seconds * 0.7 + ember as f64 / EMBERS as f64).rem_euclid(1.0) as f32;
            let position = ground + Vec3::new(
                (seconds * 2.3 + ember as f64 * 1.7).sin() as f32 * age * 0.35,
                0.55 + age * 1.8,
                (seconds * 1.7 + ember as f64).cos() as f32 * age * 0.25,
            );
            let colour = LinearRgb::new(1.0, 0.36, 0.045).with_alpha((1.0-age) * 0.75);
            for offset in [-right * 0.018, right * 0.018, Vec3::Y * 0.05] {
                self.effects.push(EffectVertex::new(position + offset, colour));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_game::{Game, GameScene};

    #[test]
    fn rebuilding_tracks_scene_eviction_and_does_not_accumulate_geometry() {
        let a = TorchSpec::new(PointLight::new(
            Vec3::new(-3.0, 2.0, 0.0),
            LinearRgb::new(1.0, 0.0, 0.0),
            20.0,
            12.0,
        ))
        .unwrap();
        let b = TorchSpec::new(PointLight::new(
            Vec3::new(3.0, 4.0, 0.0),
            LinearRgb::new(0.0, 0.0, 1.0),
            20.0,
            12.0,
        ))
        .unwrap();
        let mut game = Game::empty();
        let first = game
            .load_scene(&GameScene {
                torches: vec![a],
                ..GameScene::default()
            })
            .unwrap();
        let second = game
            .load_scene(&GameScene {
                torches: vec![b],
                ..GameScene::default()
            })
            .unwrap();
        let mut view = Torches::default();
        for _ in 0..2 {
            view.rebuild(game.torches(), &[], 0.0);
            assert_eq!(view.lights(), &[a.light(), b.light()]);
            assert_eq!(view.stand().len(), 2);
            assert_eq!(view.effects().len(), 2 * FLAME_VERTICES);
        }
        assert!(game.evict_scene(first));
        view.rebuild(game.torches(), &[], 0.0);
        assert_eq!(view.lights(), &[b.light()]);
        assert_eq!(view.stand().len(), 1);
        assert_eq!(view.effects().len(), FLAME_VERTICES);
        assert!(game.evict_scene(second));
        view.rebuild(game.torches(), &[], 0.0);
        assert!(view.lights().is_empty());
        assert!(view.stand().is_empty());
        assert!(view.effects().is_empty());
    }

    #[test]
    fn all_campfires_and_attack_effects_fit_and_rebuild_from_time() {
        let torch = TorchSpec::new(PointLight::new(Vec3::Y, LinearRgb::WHITE, 20.0, 12.0)).unwrap();
        let attacks = vec![EffectVertex::new(Vec3::ZERO, LinearRgb::WHITE.with_alpha(1.0)); crate::attack_effects::MAX_ATTACK_VERTICES];
        let mut view = Torches::default();
        view.rebuild(std::iter::repeat_n(torch, MAX_POINT_LIGHTS), &attacks, 2.0);
        assert!(view.effects().len() <= arpg_gfx::MAX_EFFECT_VERTICES);
        let positions: Vec<_> = view.effects().iter().map(|v| v.position()).collect();
        view.rebuild(std::iter::repeat_n(torch, MAX_POINT_LIGHTS), &attacks, 2.0);
        assert_eq!(positions, view.effects().iter().map(|v| v.position()).collect::<Vec<_>>());
        view.rebuild(std::iter::once(torch), &[], 2.1);
        assert_eq!(view.effects().len(), FLAME_VERTICES);
        assert_ne!(view.effects()[2].position(), positions[2]);
    }
}
