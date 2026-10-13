//! Scene-owned environment illumination. Most recently installed live scene wins;
//! eviction reveals the preceding scene, and an empty game uses neutral daylight.
use arpg_core::{EnvironmentLight, LinearRgb, Report};
use arpg_sim::{Fnv, SceneId};
use serde::{Deserialize, Serialize};

/// Complete validated environment illumination, using linear RGB colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "RawEnvironment", into = "RawEnvironment")]
pub struct EnvironmentSpec { light: EnvironmentLight, storm: bool }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawEnvironment {
    #[serde(default)]
    storm: bool,
    ambient_colour: (f32, f32, f32),
    ambient_intensity: f32,
    directional_colour: (f32, f32, f32),
    directional_intensity: f32,
    direction: (f32, f32, f32),
}

impl TryFrom<RawEnvironment> for EnvironmentSpec {
    type Error = &'static str;
    fn try_from(raw: RawEnvironment) -> Result<Self, Self::Error> {
        Ok(Self { storm: raw.storm, light: EnvironmentLight::try_new(LinearRgb::try_new(raw.ambient_colour.0, raw.ambient_colour.1, raw.ambient_colour.2)?, raw.ambient_intensity,
            LinearRgb::try_new(raw.directional_colour.0, raw.directional_colour.1, raw.directional_colour.2)?, raw.directional_intensity, raw.direction.into())? })
    }
}

impl From<EnvironmentSpec> for RawEnvironment {
    fn from(spec: EnvironmentSpec) -> Self {
        let light = spec.light();
        Self { storm: spec.storm(), ambient_colour: light.ambient_colour().to_vec3().into(), ambient_intensity: light.ambient_intensity(),
            directional_colour: light.directional_colour().to_vec3().into(), directional_intensity: light.directional_intensity(),
            direction: light.direction().into() }
    }
}

impl EnvironmentSpec {
    /// Wraps the shared validated light; there are no additional gameplay limits.
    pub fn new(light: EnvironmentLight) -> Self { Self { light, storm: false } }
    /// Selects presentation-only rain and lightning for this scene.
    #[must_use]
    pub fn with_storm(self, storm: bool) -> Self { Self { storm, ..self } }
    /// Whether the scene requests rain and lightning; never a gameplay input.
    pub fn storm(self) -> bool { self.storm }
    /// Immutable illumination consumed by presentation.
    pub fn light(self) -> EnvironmentLight { self.light }
}

#[derive(Default)]
pub(crate) struct Environments {
    rows: Vec<(SceneId, EnvironmentSpec)>,
}

impl Environments {
    pub(crate) fn install(&mut self, owner: SceneId, spec: EnvironmentSpec) { self.rows.push((owner, spec)); }
    pub(crate) fn evict(&mut self, owner: SceneId) { self.rows.retain(|row| row.0 != owner); }
    pub(crate) fn active(&self) -> EnvironmentLight {
        self.rows.last().map_or_else(EnvironmentLight::default, |row| row.1.light())
    }
    pub(crate) fn storm(&self) -> bool { self.rows.last().is_some_and(|row| row.1.storm()) }
    pub(crate) fn hash(&self, hash: &mut Fnv) {
        hash.usize(self.rows.len());
        for (owner, spec) in &self.rows {
            owner.hash_into(hash);
            hash.u64(u64::from(spec.storm()));
            let light = spec.light();
            for value in light.ambient_colour().to_array().into_iter()
                .chain([light.ambient_intensity()]).chain(light.directional_colour().to_array())
                .chain([light.directional_intensity()]).chain(light.direction().to_array()) {
                hash.f32(value);
            }
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("environment", |out| {
            if let Some((owner, _)) = self.rows.last() { out.text("scene", &owner.to_string()); }
            out.bool("storm", self.storm());
            report_light(self.active(), out);
        });
        out.object("scene_environments", |out| {
            for (owner, spec) in &self.rows {
                out.object(&owner.to_string(), |out| { out.bool("storm", spec.storm()); report_light(spec.light(), out); });
            }
        });
    }
}

fn report_light(light: EnvironmentLight, out: &mut Report) {
    out.vec3("ambient_colour", light.ambient_colour().to_vec3());
    out.num("ambient_intensity", light.ambient_intensity());
    out.vec3("directional_colour", light.directional_colour().to_vec3());
    out.num("directional_intensity", light.directional_intensity());
    out.vec3("direction", light.direction());
}
