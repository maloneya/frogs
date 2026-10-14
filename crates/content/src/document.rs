//! Scene-local editor metadata and relationship-safe authored removals.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use arpg_game::GameScene;

/// Physical or decorative collection containing an authored object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneObjectKind {
    /// Explicit body, including doors, chests and activation props.
    Body,
    /// Static rectangular wall.
    Wall,
    /// Repeating body source.
    Source,
    /// One-time rectangular body layout.
    Grid,
    /// Tree and its rooted physical trunk.
    Tree,
    /// Stationary point light and campfire decoration.
    Campfire,
}

/// Scene-local authored identity; indices follow their owning collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneObjectRef {
    /// Owning collection.
    pub kind: SceneObjectKind,
    /// Index within that collection.
    pub index: usize,
}

/// Optional editor name. Labels do not participate in gameplay or its hash.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneObjectLabel {
    /// Named authored object.
    pub target: SceneObjectRef,
    /// Unique trimmed printable name, 1–64 characters.
    pub name: String,
}

fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.chars().count() > 64 || name.trim() != name
        || name.chars().any(char::is_control)
    {
        return Err("object names must be trimmed, printable, and 1–64 characters");
    }
    Ok(())
}

impl SceneDocument {
    /// Validate metadata and admit the complete playable scene through Game.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_labels()?;
        arpg_game::Game::from_scene(&self.scene).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn object_exists(&self, target: SceneObjectRef) -> bool {
        let count = match target.kind {
            SceneObjectKind::Body => self.scene.engine.bodies.len(),
            SceneObjectKind::Wall => self.scene.engine.walls.len(),
            SceneObjectKind::Source => self.scene.engine.sources.len(),
            SceneObjectKind::Grid => self.scene.engine.grids.len(),
            SceneObjectKind::Tree => self.scene.trees.len(),
            SceneObjectKind::Campfire => self.scene.torches.len(),
        };
        target.index < count
    }

    /// Validate names independently of gameplay admission.
    pub fn validate_labels(&self) -> Result<(), &'static str> {
        let mut targets = BTreeSet::new();
        let mut names = BTreeSet::new();
        for label in &self.labels {
            validate_name(&label.name)?;
            if !self.object_exists(label.target) {
                return Err("named object no longer exists");
            }
            if !targets.insert(label.target) || !names.insert(label.name.as_str()) {
                return Err("object names and their targets must be unique");
            }
        }
        Ok(())
    }

    /// Read the optional editor name of an authored object.
    pub fn object_name(&self, target: SceneObjectRef) -> Option<&str> {
        self.labels.iter().find(|label| label.target == target).map(|label| label.name.as_str())
    }

    /// Rename an existing object, or clear its name with an empty string.
    /// Refused names leave all metadata unchanged.
    pub fn set_object_name(&mut self, target: SceneObjectRef, name: &str) -> Result<(), &'static str> {
        if !self.object_exists(target) { return Err("named object no longer exists"); }
        if !name.is_empty() {
            validate_name(name)?;
            if self.labels.iter().any(|label| label.target != target && label.name == name) {
                return Err("object names must be unique");
            }
        }
        if let Some(index) = self.labels.iter().position(|label| label.target == target) {
            if name.is_empty() { self.labels.remove(index); }
            else { self.labels[index].name = name.into(); }
        } else if !name.is_empty() {
            self.labels.push(SceneObjectLabel { target, name: name.into() });
        }
        Ok(())
    }

    /// After removing an object, discard its label and compact surviving indices.
    /// Body/source removal calls this itself; other collection editors must call
    /// it once after their successful removal. To clear only a name, use
    /// [`Self::set_object_name`] with an empty string.
    pub fn remove_object_label(&mut self, target: SceneObjectRef) {
        self.labels.retain(|label| label.target != target);
        for label in &mut self.labels {
            if label.target.kind == target.kind && label.target.index > target.index {
                label.target.index -= 1;
            }
        }
    }

}

/// Persisted authoring document. Runtime consumers load only `scene`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneDocument {
    /// Complete game-owned playable data.
    pub scene: GameScene,
    /// Optional authoring names, never installed into Game.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<SceneObjectLabel>,
}
impl SceneDocument {
    /// Remove a body while preserving gameplay links and authoring labels.
    pub fn remove_body(&mut self, index: usize) -> Result<(), &'static str> {
        self.scene.remove_body(index)?;
        self.remove_object_label(SceneObjectRef { kind: SceneObjectKind::Body, index });
        Ok(())
    }
    /// Remove a source while preserving gameplay links and authoring labels.
    pub fn remove_source(&mut self, index: usize) -> Result<(), &'static str> {
        self.scene.remove_source(index)?;
        self.remove_object_label(SceneObjectRef { kind: SceneObjectKind::Source, index });
        Ok(())
    }
}
