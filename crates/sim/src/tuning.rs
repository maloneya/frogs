//! Type-wide runtime tuning. Admission, defaults and UI vocabulary live together.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An editable gameplay attribute, in stable display and serialization order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum TuningField {
    /// Powered player movement.
    PlayerSpeed,
    /// Maximum powered player acceleration, also bounding braking effort.
    PlayerAcceleration,
    /// Player mass; effort at the acceleration limit is mass times acceleration.
    PlayerMass,
    /// Momentum imparted by Cleave.
    CleaveImpulse,
    /// Momentum imparted by Slam.
    SlamImpulse,
    /// Stiffness of overlap correction, as a spring frequency.
    ContactFrequency,
    /// Damping relative to critical damping in overlap correction.
    ContactDamping,
    /// Fastest speed at which overlap correction separates bodies.
    ContactPushSpeed,
}

/// Display and admission facts for one field.
pub struct TuningAttribute {
    /// Stable harness/report key.
    pub key: &'static str,
    /// Entity or ability category.
    pub group: &'static str,
    /// Human-readable attribute.
    pub label: &'static str,
    /// Physical units.
    pub unit: &'static str,
    /// Built-in value.
    pub default: f32,
    /// Minimum accepted value.
    pub min: f32,
    /// Maximum accepted value.
    pub max: f32,
    /// Increment for arrow buttons.
    pub step: f32,
}

impl TuningField {
    /// Complete catalog shared by all editing surfaces.
    pub const ALL: [Self; 8] = [
        Self::PlayerSpeed,
        Self::PlayerAcceleration,
        Self::PlayerMass,
        Self::CleaveImpulse,
        Self::SlamImpulse,
        Self::ContactFrequency,
        Self::ContactDamping,
        Self::ContactPushSpeed,
    ];

    /// Presentation and validation metadata from the owning definition.
    pub fn attribute(self) -> TuningAttribute {
        let (key, group, label, unit, default, min, max, step) = match self {
            Self::PlayerSpeed => (
                "player_speed",
                "Player",
                "Movement speed",
                "m/s",
                9.0,
                0.0,
                crate::pass::motion::MAX_MOTOR_SPEED,
                0.1,
            ),
            Self::PlayerAcceleration => (
                "player_acceleration",
                "Player",
                "Motor acceleration",
                "m/s²",
                60.0,
                0.0,
                crate::pass::motion::MAX_MOTOR_ACCELERATION,
                1.0,
            ),
            Self::PlayerMass => (
                "player_mass",
                "Player",
                "Mass",
                "kg",
                20.0,
                crate::Mass::MIN,
                crate::Mass::MAX,
                0.1,
            ),
            Self::CleaveImpulse => (
                "cleave_impulse",
                "Cleave",
                "Attack impulse",
                "kg m/s",
                6.0,
                0.1,
                1000.0,
                1.0,
            ),
            Self::SlamImpulse => (
                "slam_impulse",
                "Slam",
                "Attack impulse",
                "kg m/s",
                72.0,
                0.1,
                1000.0,
                1.0,
            ),
            // The solver caps stiffness at an eighth of its substep rate for
            // stability, so values above that cap would have no effect.
            Self::ContactFrequency => (
                "contact_frequency",
                "Contacts",
                "Contact frequency",
                "Hz",
                30.0,
                1.0,
                crate::solver::MAX_CONTACT_HERTZ,
                1.0,
            ),
            Self::ContactDamping => (
                "contact_damping",
                "Contacts",
                "Damping ratio",
                "ratio",
                10.0,
                0.0,
                20.0,
                0.5,
            ),
            Self::ContactPushSpeed => (
                "contact_push_speed",
                "Contacts",
                "Overlap push speed",
                "m/s",
                3.0,
                0.1,
                20.0,
                0.1,
            ),
        };
        TuningAttribute {
            key,
            group,
            label,
            unit,
            default,
            min,
            max,
            step,
        }
    }

    /// Resolves a harness key without a second string catalog.
    pub fn named(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|field| field.attribute().key == key)
    }
}

/// Complete validated settings. Files must supply every field; unknown fields are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "TuningFile", into = "BTreeMap<TuningField, f32>")]
pub struct Tuning([f32; TuningField::ALL.len()]);

impl Default for Tuning {
    fn default() -> Self {
        Self(TuningField::ALL.map(|field| field.attribute().default))
    }
}

/// Validated global contact material, sampled once for a physics step.
/// Construction stays with `Tuning`; producers cannot override contact response.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContactMaterial {
    frequency_hz: f32,
    damping_ratio: f32,
    push_speed: f32,
}
impl ContactMaterial {
    pub(crate) fn frequency_hz(self) -> f32 {
        self.frequency_hz
    }
    pub(crate) fn damping_ratio(self) -> f32 {
        self.damping_ratio
    }
    pub(crate) fn push_speed(self) -> f32 {
        self.push_speed
    }
}

impl Tuning {
    pub(crate) fn contact_material(self) -> ContactMaterial {
        ContactMaterial {
            frequency_hz: self.get(TuningField::ContactFrequency),
            damping_ratio: self.get(TuningField::ContactDamping),
            push_speed: self.get(TuningField::ContactPushSpeed),
        }
    }

    /// Current value of an attribute.
    pub fn get(self, field: TuningField) -> f32 {
        self.0[field as usize]
    }

    /// Validates one edit without mutating the original settings.
    pub fn with(self, field: TuningField, value: f32) -> Result<Self, String> {
        let attr = field.attribute();
        if !value.is_finite() || !(attr.min..=attr.max).contains(&value) {
            return Err(format!(
                "{} must be finite and between {} and {} {}",
                attr.label, attr.min, attr.max, attr.unit
            ));
        }
        let mut next = self;
        next.0[field as usize] = if value == 0.0 { 0.0 } else { value };
        Ok(next)
    }

    pub(crate) fn hash(self, hash: &mut crate::Fnv) {
        for value in self.0 {
            hash.f32(value);
        }
    }

    /// Reports accepted runtime values using the catalog's stable keys.
    pub fn report(self, out: &mut arpg_core::Report) {
        for field in TuningField::ALL {
            out.num(field.attribute().key, self.get(field));
        }
    }
}

impl TryFrom<BTreeMap<TuningField, f32>> for Tuning {
    type Error = String;
    fn try_from(fields: BTreeMap<TuningField, f32>) -> Result<Self, Self::Error> {
        let mut tuning = Self::default();
        for field in TuningField::ALL {
            tuning = tuning.with(
                field,
                *fields
                    .get(&field)
                    .ok_or_else(|| format!("missing {}", field.attribute().key))?,
            )?;
        }
        Ok(tuning)
    }
}

impl From<Tuning> for BTreeMap<TuningField, f32> {
    fn from(tuning: Tuning) -> Self {
        TuningField::ALL
            .into_iter()
            .map(|field| (field, tuning.get(field)))
            .collect()
    }
}

struct TuningFile(BTreeMap<TuningField, f32>);

impl<'de> Deserialize<'de> for TuningFile {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = TuningFile;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a complete tuning map with unique fields")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut fields = BTreeMap::new();
                while let Some((field, value)) = map.next_entry()? {
                    if fields.insert(field, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate tuning field"));
                    }
                }
                Ok(TuningFile(fields))
            }
        }
        decoder.deserialize_map(Visitor)
    }
}

impl TryFrom<TuningFile> for Tuning {
    type Error = String;
    fn try_from(file: TuningFile) -> Result<Self, Self::Error> {
        Self::try_from(file.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_material_samples_only_validated_global_settings() {
        let defaults = Tuning::default().contact_material();
        assert_eq!(defaults.frequency_hz(), 30.0);
        assert_eq!(defaults.damping_ratio(), 10.0);
        assert_eq!(defaults.push_speed(), 3.0);
        let tuning = Tuning::default()
            .with(TuningField::ContactFrequency, 12.0)
            .unwrap()
            .with(TuningField::ContactDamping, 0.0)
            .unwrap()
            .with(TuningField::ContactPushSpeed, 0.5)
            .unwrap();
        let material = tuning.contact_material();
        assert_eq!(material.frequency_hz(), 12.0);
        assert_eq!(material.damping_ratio(), 0.0);
        assert_eq!(material.push_speed(), 0.5);
        for field in [
            TuningField::ContactFrequency,
            TuningField::ContactDamping,
            TuningField::ContactPushSpeed,
        ] {
            let attribute = field.attribute();
            assert!(tuning.with(field, attribute.min).is_ok());
            assert!(tuning.with(field, attribute.max).is_ok());
            for invalid in [
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                attribute.min - attribute.step,
                attribute.max + attribute.step,
            ] {
                assert!(tuning.with(field, invalid).is_err());
                assert_eq!(tuning.contact_material(), material);
            }
        }
    }
}
