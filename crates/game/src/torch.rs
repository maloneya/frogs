//! Scene-owned stationary torches; no collision or tick-time behaviour.

use arpg_core::{MAX_POINT_LIGHTS, PointLight, Report};
use arpg_sim::{Fnv, SceneId};
use serde::Deserialize;

/// One authored torch. Its position is the illumination centre in world metres.
/// Construction and deserialization share validation; fields cannot bypass it.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(try_from = "RawTorch")]
pub struct TorchSpec(PointLight);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTorch {
    position: (f32, f32, f32),
    colour: (f32, f32, f32),
    intensity: f32,
    range: f32,
}

impl TryFrom<RawTorch> for TorchSpec {
    type Error = &'static str;
    fn try_from(raw: RawTorch) -> Result<Self, Self::Error> {
        Self::new(PointLight::try_new(
            raw.position.into(),
            raw.colour.into(),
            raw.intensity,
            raw.range,
        )?)
    }
}

impl TorchSpec {
    /// Requires ground clearance for the illumination centre.
    pub fn new(light: PointLight) -> Result<Self, &'static str> {
        if light.position().y < 0.3 {
            return Err("torch flame must be at least 0.3 metres above ground");
        }
        Ok(Self(light))
    }

    /// The validated illumination settings, also used for the visible torch.
    pub fn light(self) -> PointLight {
        self.0
    }

}

struct Torch {
    owner: SceneId,
    spec: TorchSpec,
}

#[derive(Default)]
pub(crate) struct Torches {
    rows: Vec<Torch>,
}

impl Torches {
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn validate_add(&self, specs: &[TorchSpec]) -> Result<(), crate::SceneError> {
        if specs.len() > MAX_POINT_LIGHTS - self.rows.len() {
            return Err(crate::SceneError::Lighting(
                "scene would exceed the live point-light limit",
            ));
        }
        Ok(())
    }

    pub(crate) fn install(&mut self, owner: SceneId, specs: &[TorchSpec]) {
        self.validate_add(specs)
            .expect("validated before physical admission");
        self.rows
            .extend(specs.iter().map(|&spec| Torch { owner, spec }));
    }

    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|row| row.owner != owner);
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = TorchSpec> + '_ {
        self.rows.iter().map(|row| row.spec)
    }

    pub(crate) fn state(&self, owner: SceneId, nth: usize) -> Option<TorchSpec> {
        self.rows
            .iter()
            .filter(|row| row.owner == owner)
            .nth(nth)
            .map(|row| row.spec)
    }

    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.len());
        for Torch { owner, spec } in &self.rows {
            owner.hash_into(out);
            let light = spec.light();
            for value in light
                .position()
                .to_array()
                .into_iter()
                .chain(light.colour().to_array())
                .chain([light.intensity(), light.range()])
            {
                out.f32(value);
            }
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.int("torch_count", self.len() as u64);
        out.object("torches", |out| {
            // Installation appends each scene contiguously; eviction preserves order.
            for scene in self.rows.chunk_by(|a, b| a.owner == b.owner) {
                for (nth, Torch { owner, spec }) in scene.iter().enumerate() {
                    out.object(&format!("{owner}/{nth}"), |out| {
                        let light = spec.light();
                        out.text("scene", &owner.to_string());
                        out.vec3("position", light.position());
                        out.vec3("colour", light.colour());
                        out.num("intensity", light.intensity());
                        out.num("range", light.range());
                    });
                }
            }
        });
    }
}
