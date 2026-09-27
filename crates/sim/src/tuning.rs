//! Type-wide runtime tuning. Admission, defaults and UI vocabulary live together.
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

/// An editable gameplay attribute, in stable display and serialization order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum TuningField {
    /// Powered player movement.
    PlayerSpeed,
    /// Momentum imparted by Cleave.
    CleaveImpulse,
    /// Momentum imparted by Slam.
    SlamImpulse,
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
    pub const ALL: [Self; 3] = [Self::PlayerSpeed, Self::CleaveImpulse, Self::SlamImpulse];

    /// Presentation and validation metadata from the owning definition.
    pub fn attribute(self) -> TuningAttribute {
        let (key, group, label, default) = match self {
            Self::PlayerSpeed => ("player_speed", "Player", "Movement speed", 9.0),
            Self::CleaveImpulse => ("cleave_impulse", "Cleave", "Attack impulse", 6.0),
            Self::SlamImpulse => ("slam_impulse", "Slam", "Attack impulse", 72.0),
        };
        let impulse = matches!(self, Self::CleaveImpulse | Self::SlamImpulse);
        TuningAttribute { key, group, label, default, unit: if impulse { "kg m/s" } else { "m/s" },
            // Limits keep the first tuning surface within modest physical workloads.
            // Zero speed is useful for isolating combat; resolved attacks require positive impulse.
            min: if impulse { 0.1 } else { 0.0 }, max: if impulse { 1000.0 } else { 60.0 },
            step: if impulse { 1.0 } else { 0.1 } }
    }

    /// Resolves a harness key without a second string catalog.
    pub fn named(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.attribute().key == key)
    }
}

/// Complete validated settings. Files must supply every field; unknown fields are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "TuningFile", into = "BTreeMap<TuningField, f32>")]
pub struct Tuning([f32; 3]);

impl Default for Tuning {
    fn default() -> Self { Self(TuningField::ALL.map(|field| field.attribute().default)) }
}

impl Tuning {
    /// Current value of an attribute.
    pub fn get(self, field: TuningField) -> f32 { self.0[field as usize] }

    /// Validates one edit without mutating the original settings.
    pub fn with(self, field: TuningField, value: f32) -> Result<Self, String> {
        let attr = field.attribute();
        if !value.is_finite() || !(attr.min..=attr.max).contains(&value) {
            return Err(format!("{} must be finite and between {} and {} {}", attr.label, attr.min, attr.max, attr.unit));
        }
        let mut next = self;
        next.0[field as usize] = if value == 0.0 { 0.0 } else { value };
        Ok(next)
    }

    pub(crate) fn hash(self, hash: &mut crate::Fnv) {
        for value in self.0 { hash.f32(value); }
    }

    /// Reports accepted runtime values using the catalog's stable keys.
    pub fn report(self, out: &mut arpg_core::Report) {
        for field in TuningField::ALL { out.num(field.attribute().key, self.get(field)); }
    }
}

impl TryFrom<BTreeMap<TuningField, f32>> for Tuning {
    type Error = String;
    fn try_from(fields: BTreeMap<TuningField, f32>) -> Result<Self, Self::Error> {
        let mut tuning = Self::default();
        for field in TuningField::ALL {
            tuning = tuning.with(field, *fields.get(&field).ok_or_else(|| format!("missing {}", field.attribute().key))?)?;
        }
        Ok(tuning)
    }
}

impl From<Tuning> for BTreeMap<TuningField, f32> {
    fn from(tuning: Tuning) -> Self {
        TuningField::ALL.into_iter().map(|field| (field, tuning.get(field))).collect()
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
            fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
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
    fn try_from(file: TuningFile) -> Result<Self, Self::Error> { Self::try_from(file.0) }
}
