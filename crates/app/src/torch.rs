//! Presentation geometry rebuilt from the game's scene-owned torches.
use arpg_core::{Instance, MAX_POINT_LIGHTS, PointLight};
use arpg_game::TorchSpec;
use arpg_gfx::EffectVertex;
use glam::Vec3;

const FLAME_VERTICES: usize = 48;
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
            stands: Vec::with_capacity(MAX_POINT_LIGHTS * 2),
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
    ) {
        self.lights.clear();
        self.stands.clear();
        self.effects.clear();
        for torch in torches {
            assert!(self.lights.len() < MAX_POINT_LIGHTS, "scene light capacity");
            let light = torch.light();
            self.lights.push(light);
            let ground = Vec3::new(light.position().x, 0.0, light.position().z);
            // The shared prop is 0.5 metres high with its origin at ground level.
            self.stands.push(Instance::new(
                ground,
                Vec3::new(1.2, 0.4, 1.2),
                Vec3::splat(0.18),
            ));
            self.stands.push(Instance::new(
                ground,
                Vec3::new(0.28, (light.position().y - 0.25) * 2.0, 0.28),
                Vec3::new(0.22, 0.10, 0.035),
            ));
            // A small faceted flame, visible from all sides. Emission is visual;
            // only the PointLight illuminates other surfaces.
            let centre = light.position();
            for (radius, bottom, top, colour) in [
                (0.20, -0.26, 0.38, light.colour().extend(1.0)),
                (
                    0.11,
                    -0.27,
                    0.10,
                    light.colour().lerp(Vec3::ONE, 0.55).extend(1.0),
                ),
            ] {
                let ring = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z];
                for i in 0..4 {
                    let a = centre + ring[i] * radius;
                    let b = centre + ring[(i + 1) % 4] * radius;
                    for point in [
                        centre + Vec3::Y * top,
                        b,
                        a,
                        centre + Vec3::Y * bottom,
                        a,
                        b,
                    ] {
                        self.effects.push(EffectVertex::new(point, colour));
                    }
                }
            }
        }
        assert_eq!(self.effects.len(), self.lights.len() * FLAME_VERTICES);
        self.effects.extend_from_slice(attacks);
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
            Vec3::X,
            20.0,
            12.0,
        ))
        .unwrap();
        let b = TorchSpec::new(PointLight::new(
            Vec3::new(3.0, 4.0, 0.0),
            Vec3::Z,
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
            view.rebuild(game.torches(), &[]);
            assert_eq!(view.lights(), &[a.light(), b.light()]);
            assert_eq!(view.stand().len(), 4);
            assert_eq!(view.effects().len(), 2 * FLAME_VERTICES);
        }
        assert!(game.evict_scene(first));
        view.rebuild(game.torches(), &[]);
        assert_eq!(view.lights(), &[b.light()]);
        assert_eq!(view.stand().len(), 2);
        assert_eq!(view.effects().len(), FLAME_VERTICES);
        assert!(game.evict_scene(second));
        view.rebuild(game.torches(), &[]);
        assert!(view.lights().is_empty());
        assert!(view.stand().is_empty());
        assert!(view.effects().is_empty());
    }
}
