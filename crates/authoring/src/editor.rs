//! Stateless authoring operations. The companion retains drafts/undo; each
//! request carries a complete authoring document, never a live-world snapshot.
use arpg_core::{EnvironmentLight, LinearRgb, PointLight};
use arpg_game::{EnvironmentSpec, TorchSpec, TreeKind, TreeSpec, EnemyAttackSpec};
use arpg_content::{SceneObjectKind as Kind, SceneObjectRef as Target};
use arpg_content::SceneDocument;
mod encounters;
#[cfg(test)]
mod authoring_tests;
pub use encounters::{EncounterDraft, PropKind};
use arpg_sim::{BodyGrid, Condition, Placed, SourceSpec, Template, Wall};
use glam::{Vec2, Vec3};
use serde::Deserialize;
use serde_json::{Number, Value, json};
#[cfg(test)]
use arpg_game::Game;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
/// Stateless document operations, also decoded by the JSON CLI.
pub enum Request {
    /// Create an empty document.
    New,
    /// Read a document and its current disk revision.
    Open { #[doc = "Scene file path."] path: PathBuf },
    /// Apply a single typed edit.
    Edit { #[doc = "Complete authoring document."] document: SceneDocument, #[doc = "Typed scene edit."] edit: Edit },
    /// Validate and persist a document without contacting the game.
    Save { #[doc = "Complete authoring document."] document: SceneDocument, #[doc = "Scene file path."] path: PathBuf, #[doc = "Revision read on open; None creates a new file only."] revision: Option<String> },
    /// Validate a complete authored document.
    Validate { #[doc = "Complete authoring document."] document: SceneDocument },
    /// Apply an ordered transaction of edits without writing files.
    Batch { #[doc = "Complete authoring document."] document: SceneDocument, #[doc = "Edits in application order."] edits: Vec<Edit> },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
/// One authored scene change, validated before returning the next document.
pub enum Edit {
    /// Place a default object.
    Add { #[doc = "Object collection or prop preset."] kind: Kind, #[doc = "World X/Z position in metres."] pos: (f32, f32) },
    /// Place a gameplay prop preset.
    Place { #[doc = "Object collection or prop preset."] kind: PropKind, #[doc = "World X/Z position in metres."] pos: (f32, f32) },
    /// Set an object name, or clear it with an empty string.
    Name { #[doc = "Scene-local authored object."] target: Target, #[doc = "Display name."] name: String },
    /// Create or replace one complete encounter.
    Encounter { #[doc = "Existing encounter index; None creates one."] index: Option<usize>, #[doc = "New field value or complete encounter specification."] value: EncounterDraft },
    /// Remove an encounter while retaining its objects.
    DeleteEncounter { #[doc = "Existing encounter index."] index: usize },
    /// Place a wall between opposite corners.
    DrawWall { #[doc = "First world-space corner."] start: (f32, f32), #[doc = "Opposite world-space corner."] end: (f32, f32) },
    /// Move an authored object.
    Move { #[doc = "Scene-local authored object."] target: Target, #[doc = "World X/Z position in metres."] pos: (f32, f32) },
    /// Set a scalar inspector field.
    Set { #[doc = "Scene-local authored object."] target: Target, #[doc = "Key from the derived inspector fields."] field: String, #[doc = "Numeric value, preserving integer precision."] value: Number },
    /// Set a campfire light colour.
    Colour { #[doc = "Scene-local authored object."] target: Target, #[doc = "Linear RGB colour."] colour: (f32, f32, f32) },
    /// Set a scene illumination field.
    Lighting { #[doc = "Key from the derived inspector fields."] field: String, #[doc = "New field value or complete encounter specification."] value: f32 },
    /// Set a scene illumination colour.
    LightingColour { #[doc = "Key from the derived inspector fields."] field: String, #[doc = "Linear RGB colour."] colour: (f32, f32, f32) },
    /// Copy a placement without its encounter bindings.
    Duplicate { #[doc = "Scene-local authored object."] target: Target, #[doc = "World X/Z duplication offset."] offset: (f32, f32) },
    /// Remove an unreferenced placement.
    Delete { #[doc = "Scene-local authored object."] target: Target },
    /// Set the scene display name.
    Rename { #[doc = "Display name."] name: String },
}

fn missing() -> String { "The selected object no longer exists.".into() }

fn position(scene: &SceneDocument, target: Target) -> Result<(f32, f32), String> {
    match target.kind {
        Kind::Body => scene.scene.engine.bodies.get(target.index).map(|v| v.pos),
        Kind::Wall => scene.scene.engine.walls.get(target.index).map(|v| v.centre().into()),
        Kind::Source => scene.scene.engine.sources.get(target.index).map(|v| v.pos),
        Kind::Grid => scene.scene.engine.grids.get(target.index).map(|v| v.origin),
        Kind::Tree => scene.scene.trees.get(target.index).map(|v| v.position().into()),
        Kind::Campfire => scene.scene.torches.get(target.index).map(|v| (v.light().position().x, v.light().position().z)),
    }.ok_or_else(missing)
}

fn move_to(scene: &mut SceneDocument, target: Target, pos: (f32, f32)) -> Result<(), String> {
    if !Vec2::from(pos).is_finite() { return Err("Position must be finite.".into()); }
    match target.kind {
        Kind::Body => scene.scene.engine.bodies.get_mut(target.index).ok_or_else(missing)?.pos = pos,
        Kind::Source => scene.scene.engine.sources.get_mut(target.index).ok_or_else(missing)?.pos = pos,
        Kind::Grid => scene.scene.engine.grids.get_mut(target.index).ok_or_else(missing)?.origin = pos,
        Kind::Wall => {
            let wall = scene.scene.engine.walls.get_mut(target.index).ok_or_else(missing)?;
            *wall = Wall::new(pos.into(), wall.size(), wall.yaw())?;
        }
        Kind::Tree => {
            let tree = scene.scene.trees.get_mut(target.index).ok_or_else(missing)?;
            *tree = TreeSpec::new(tree.kind(), pos.into(), tree.yaw(), tree.scale())?;
        }
        Kind::Campfire => {
            let torch = scene.scene.torches.get_mut(target.index).ok_or_else(missing)?;
            let light = torch.light();
            *torch = TorchSpec::new(PointLight::try_new(Vec3::new(pos.0, light.position().y, pos.1), light.colour(), light.intensity(), light.range())?)?;
        }
    }
    Ok(())
}

fn integer<T: TryFrom<u64>>(value: &Number) -> Result<T, String> {
    let whole = value.as_u64().or_else(|| {
        // Whole-valued floating JSON numbers are accepted only in their exact range.
        let n = value.as_f64()?;
        ((0.0..=9_007_199_254_740_991.0).contains(&n) && n.fract() == 0.0).then_some(n as u64)
    }).ok_or("Enter a nonnegative whole number.")?;
    T::try_from(whole).map_err(|_| "Whole number exceeds this field's range.".into())
}

fn template_field(template: &mut Template, field: &str, value: f64) -> Result<(), String> {
    if value != 0.0 && value != 1.0 { return Err("Setting must be on or off.".into()); }
    let enabled = value == 1.0;
    let (mut fixed, mut seeks, mut interactive) = (template.fixed(), template.seeks(), template.interactable());
    match field {
        "fixed" => { fixed = enabled; if fixed { seeks = false; } }
        "seeks" => seeks = enabled,
        "interactable" => interactive = enabled,
        _ => return Err("Unknown body template field.".into()),
    }
    if fixed && seeks { return Err("Static props cannot seek.".into()); }
    let mut next = if fixed { Template::BLOCK } else { Template::BODY };
    if seeks { next = next.seeking(); }
    if interactive { next = next.interactive(); }
    *template = next;
    Ok(())
}

fn set(scene: &mut SceneDocument, target: Target, field: &str, raw: Number) -> Result<(), String> {
    let scalar = raw.as_f64().ok_or("Value must be numeric.")?;
    let value = scalar as f32;
    if !value.is_finite() { return Err("Value must be finite.".into()); }
    if field == "x" || field == "z" {
        let (x, z) = position(scene, target)?;
        return move_to(scene, target, if field == "x" { (value, z) } else { (x, value) });
    }
    match target.kind {
        Kind::Body => {
            if field == "component" { encounters::component(scene, target.index, integer::<u32>(&raw)?)?; }
            else if field == "attacker" {
                let body = scene.scene.engine.bodies.get_mut(target.index).ok_or_else(missing)?;
                if scalar == 1.0 {
                    template_field(&mut body.what, "seeks", 1.0)?;
                    scene.scene.enemy_attack = Some(EnemyAttackSpec { body: target.index });
                } else if scalar == 0.0 {
                    if scene.scene.enemy_attack.is_some_and(|a| a.body == target.index) { scene.scene.enemy_attack = None; }
                } else { return Err("Contact attack must be on or off.".into()); }
            } else { template_field(&mut scene.scene.engine.bodies.get_mut(target.index).ok_or_else(missing)?.what, field, scalar)?; }
        }
        Kind::Wall => {
            let wall = scene.scene.engine.walls.get_mut(target.index).ok_or_else(missing)?;
            let mut size = wall.size();
            let mut yaw = wall.yaw();
            match field { "width" => size.x = value, "depth" => size.y = value,
                "yaw" => yaw = value.to_radians(), _ => return Err("Unknown wall field.".into()) }
            *wall = Wall::new(wall.centre(), size, yaw)?;
        }
        Kind::Source => {
            let source = scene.scene.engine.sources.get_mut(target.index).ok_or_else(missing)?;
            match field {
                "radius" => source.radius = value,
                "every" => source.every = integer(&raw)?,
                "enabled" if scalar == 0.0 || scalar == 1.0 => source.enabled = scalar == 1.0,
                "seeks" | "fixed" | "interactable" => template_field(&mut source.what, field, scalar)?,
                "condition" => source.when = match integer::<u32>(&raw)? {
                    0 => Condition::Always, 1 => Condition::FewerThan(12), 2 => Condition::PlayerWithin(10.0),
                    _ => return Err("Unknown source condition.".into()),
                },
                "threshold" => match &mut source.when {
                    Condition::FewerThan(n) => *n = integer(&raw)?,
                    Condition::PlayerWithin(r) => *r = value,
                    Condition::Always => return Err("Always has no threshold.".into()),
                },
                _ => return Err("Unknown source field.".into()),
            }
        }
        Kind::Grid => {
            let grid = scene.scene.engine.grids.get_mut(target.index).ok_or_else(missing)?;
            match field {
                "columns" => grid.columns = integer(&raw)?,
                "rows" => grid.rows = integer(&raw)?,
                "spacing" => grid.spacing = value,
                "seeks" | "fixed" | "interactable" => template_field(&mut grid.what, field, scalar)?,
                _ => return Err("Unknown formation field.".into()),
            }
        }
        Kind::Tree => {
            let tree = scene.scene.trees.get_mut(target.index).ok_or_else(missing)?;
            let (mut kind, mut yaw, mut scale) = (tree.kind(), tree.yaw(), tree.scale());
            match field {
                "kind" => kind = match integer::<u32>(&raw)? { 0 => TreeKind::Pine, 1 => TreeKind::Broadleaf, 2 => TreeKind::Bare, _ => return Err("Unknown tree variant.".into()) },
                "yaw" => yaw = value.to_radians(),
                "scale" => scale = value,
                _ => return Err("Unknown tree field.".into()),
            }
            *tree = TreeSpec::new(kind, tree.position(), yaw, scale)?;
        }
        Kind::Campfire => {
            let torch = scene.scene.torches.get_mut(target.index).ok_or_else(missing)?;
            let light = torch.light();
            let (mut pos, mut intensity, mut range) = (light.position(), light.intensity(), light.range());
            match field {
                "height" => pos.y = value,
                "intensity" => intensity = value,
                "range" => range = value,
                _ => return Err("Unknown campfire field.".into()),
            }
            *torch = TorchSpec::new(PointLight::try_new(pos, light.colour(), intensity, range)?)?;
        }
    }
    Ok(())
}

fn lighting(scene: &mut SceneDocument, field: &str, value: f32) -> Result<(), String> {
    if !value.is_finite() { return Err("Value must be finite.".into()); }
    let light = scene.scene.environment.light();
    let (mut ambient, mut directional) = (light.ambient_intensity(), light.directional_intensity());
    let direction = light.direction().normalize();
    let mut azimuth = direction.x.atan2(direction.z);
    let mut elevation = direction.y.clamp(-1.0, 1.0).asin();
    match field {
        "ambient_intensity" => ambient = value,
        "directional_intensity" => directional = value,
        "azimuth" => azimuth = value.to_radians(),
        "elevation" if (-90.0..=90.0).contains(&value) => elevation = value.to_radians(),
        "elevation" => return Err("Light elevation must be between -90° and 90°.".into()),
        _ => return Err("Unknown scene lighting field.".into()),
    }
    // Brightness edits preserve the authored direction exactly; only angular
    // edits reconstruct it from the inspector's more useful controls.
    let direction = if field == "azimuth" || field == "elevation" {
        Vec3::new(elevation.cos() * azimuth.sin(), elevation.sin(), elevation.cos() * azimuth.cos())
    } else { light.direction() };
    scene.scene.environment = EnvironmentSpec::new(EnvironmentLight::try_new(light.ambient_colour(), ambient, light.directional_colour(), directional, direction)?);
    Ok(())
}

/// Consumes a draft copy; refusal cannot partially change the client's document.
/// Apply a typed operation; the caller retains its accepted document on refusal.
pub fn edit(mut scene: SceneDocument, edit: Edit) -> Result<(SceneDocument, Option<Target>), String> {
    let mut selected = None;
    match edit {
        Edit::Place { kind, pos } => selected = Some(encounters::place(&mut scene, kind, pos)),
        Edit::Name { target, name } => { scene.set_object_name(target, &name)?; selected = Some(target); }
        Edit::Encounter { index, value } => encounters::apply(&mut scene, index, value)?,
        Edit::DeleteEncounter { index } => {
            if index >= scene.scene.encounters.len() { return Err("Encounter no longer exists.".into()); }
            scene.scene.encounters.remove(index);
        }
        Edit::Lighting { field, value } => lighting(&mut scene, &field, value)?,
        Edit::LightingColour { field, colour } => {
            let light = scene.scene.environment.light();
            let (mut ambient, mut directional) = (light.ambient_colour(), light.directional_colour());
            let target = match field.as_str() {
                "ambient_colour" => &mut ambient,
                "directional_colour" => &mut directional,
                _ => return Err("Unknown lighting colour.".into()),
            };
            *target = LinearRgb::try_new(colour.0, colour.1, colour.2)?;
            scene.scene.environment = EnvironmentSpec::new(EnvironmentLight::try_new(ambient, light.ambient_intensity(), directional, light.directional_intensity(), light.direction())?);
        }
        Edit::Rename { name } => {
            if name.trim().is_empty() || name.len() > 128 { return Err("Scene name must contain 1–128 bytes.".into()); }
            scene.scene.engine.name = name;
        }
        Edit::Add { kind, pos } => {
            let index = match kind {
                Kind::Body => { scene.scene.engine.bodies.push(Placed { pos, what: Template::BODY.seeking() }); scene.scene.engine.bodies.len() - 1 }
                Kind::Wall => { scene.scene.engine.walls.push(Wall::new(pos.into(), Vec2::new(4.0, 0.4), 0.0)?); scene.scene.engine.walls.len() - 1 }
                Kind::Source => { scene.scene.engine.sources.push(SourceSpec { pos, radius: 2.0, every: 60, enabled: true, when: Condition::FewerThan(12), what: Template::BODY.seeking() }); scene.scene.engine.sources.len() - 1 }
                Kind::Grid => { scene.scene.engine.grids.push(BodyGrid { origin: pos, columns: 3, rows: 3, spacing: 1.5, what: Template::BODY.seeking() }); scene.scene.engine.grids.len() - 1 }
                Kind::Tree => { scene.scene.trees.push(TreeSpec::new(TreeKind::Pine, pos.into(), 0.0, 1.0)?); scene.scene.trees.len() - 1 }
                Kind::Campfire => { scene.scene.torches.push(TorchSpec::new(PointLight::try_new(Vec3::new(pos.0, 1.5, pos.1), LinearRgb::new(1.0, 0.45, 0.12), 24.0, 14.0)?)?); scene.scene.torches.len() - 1 }
            };
            selected = Some(Target { kind, index });
        }
        Edit::DrawWall { start, end } => {
            let start = Vec2::from(start);
            let end = Vec2::from(end);
            let wall = Wall::new((start + end) * 0.5, (end - start).abs().max(Vec2::splat(0.1)), 0.0)?;
            scene.scene.engine.walls.push(wall);
            selected = Some(Target { kind: Kind::Wall, index: scene.scene.engine.walls.len() - 1 });
        }
        Edit::Move { target, pos } => { move_to(&mut scene, target, pos)?; selected = Some(target); }
        Edit::Set { target, field, value } => { set(&mut scene, target, &field, value)?; selected = Some(target); }
        Edit::Colour { target, colour } => {
            if !matches!(target.kind, Kind::Campfire) { return Err("This object has no light colour.".into()); }
            let torch = scene.scene.torches.get_mut(target.index).ok_or_else(missing)?;
            let light = torch.light();
            *torch = TorchSpec::new(PointLight::try_new(light.position(), LinearRgb::try_new(colour.0, colour.1, colour.2)?, light.intensity(), light.range())?)?;
            selected = Some(target);
        }
        Edit::Duplicate { target, offset } => {
            let (x, z) = position(&scene, target)?;
            let index = match target.kind {
                Kind::Body => {
                    let index = scene.scene.engine.bodies.len();
                    scene.scene.engine.bodies.push(scene.scene.engine.bodies[target.index]);
                    if scene.scene.doors.iter().any(|d| d.body == target.index) { scene.scene.doors.push(arpg_game::DoorSpec { body: index }); }
                    if scene.scene.chests.iter().any(|c| c.body == target.index) { scene.scene.chests.push(arpg_game::ChestSpec { body: index }); }
                    index
                }
                Kind::Wall => { scene.scene.engine.walls.push(scene.scene.engine.walls[target.index]); scene.scene.engine.walls.len() - 1 }
                Kind::Source => { scene.scene.engine.sources.push(scene.scene.engine.sources[target.index]); scene.scene.engine.sources.len() - 1 }
                Kind::Grid => { scene.scene.engine.grids.push(scene.scene.engine.grids[target.index]); scene.scene.engine.grids.len() - 1 }
                Kind::Tree => { scene.scene.trees.push(scene.scene.trees[target.index]); scene.scene.trees.len() - 1 }
                Kind::Campfire => { scene.scene.torches.push(scene.scene.torches[target.index]); scene.scene.torches.len() - 1 }
            };
            let next = Target { kind: target.kind, index };
            move_to(&mut scene, next, (x + offset.0, z + offset.1))?;
            selected = Some(next);
        }
        Edit::Delete { target } => {
            position(&scene, target)?;
            match target.kind {
                Kind::Body => {
                    if let Some(index) = scene.scene.doors.iter().position(|d| d.body == target.index) { scene.scene.remove_door(index)?; }
                    if let Some(index) = scene.scene.chests.iter().position(|c| c.body == target.index) { scene.scene.remove_chest(index)?; }
                    if scene.scene.enemy_attack.is_some_and(|a| a.body == target.index) { scene.scene.enemy_attack = None; }
                    scene.remove_body(target.index)?;
                }
                Kind::Source => scene.remove_source(target.index)?,
                Kind::Wall => { scene.scene.engine.walls.remove(target.index); }
                Kind::Grid => { scene.scene.engine.grids.remove(target.index); }
                Kind::Tree => { scene.scene.trees.remove(target.index); }
                Kind::Campfire => { scene.scene.torches.remove(target.index); }
            }
            if !matches!(target.kind, Kind::Body | Kind::Source) { scene.remove_object_label(target); }
        }
    }
    scene.validate()?;
    Ok((scene, selected))
}

fn field(key: &str, label: &str, unit: &str, value: f32, step: f32) -> Value {
    json!({"key":key,"label":label,"unit":unit,"value":value,"step":step})
}

fn integer_field(key: &str, label: &str, unit: &str, value: impl Into<Value>) -> Value {
    json!({"key":key,"label":label,"unit":unit,"value":value.into(),"step":1,"integer":true})
}

fn template_fields(template: Template) -> Vec<Value> {
    vec![field("fixed", "Fixed prop", "bool", f32::from(template.fixed()), 1.0),
        field("seeks", "Seeks player", "bool", f32::from(template.seeks()), 1.0),
        field("interactable", "Can interact", "bool", f32::from(template.interactable()), 1.0)]
}

fn colour_field(key: &str, label: &str, colour: LinearRgb) -> Value {
    json!({"key":key,"label":label,"unit":"colour","value":0.0,"step":0.0,"colour":colour.to_array()})
}

fn target_id(target: Target) -> String {
    let kind = match target.kind { Kind::Body => "body", Kind::Wall => "wall", Kind::Source => "source", Kind::Grid => "grid", Kind::Tree => "tree", Kind::Campfire => "campfire" };
    format!("{kind}:{}", target.index)
}

/// Drawing and inspector data are derived from the authoritative description;
/// Swift carries the opaque scene back without reconstructing its schema.
pub(crate) fn document(scene: &SceneDocument, selected: Option<Target>, path: Option<PathBuf>, revision: Option<String>) -> Result<Value, String> {
    let count = scene.scene.engine.bodies.len() + scene.scene.engine.walls.len() + scene.scene.engine.sources.len() + scene.scene.engine.grids.len() + scene.scene.trees.len() + scene.scene.torches.len();
    if count > 4096 { return Err("The editor supports 4096 placements; use formations for larger crowds.".into()); }
    let mut objects = Vec::new();
    let mut push = |kind: Kind, index: usize, label: &str, pos: (f32, f32), width: f32, depth: f32, yaw: f32, radius: f32, mut fields: Vec<Value>| {
        let mut all = vec![field("x", "X", "m", pos.0, 0.5), field("z", "Z", "m", pos.1, 0.5)];
        all.append(&mut fields);
        let target = Target { kind, index };
        let fallback = format!("{label} {}", index + 1);
        let name = scene.object_name(target).unwrap_or("");
        objects.push(json!({"id":target_id(target),"kind":kind,"index":index,"label":if name.is_empty() { &fallback } else { name }, "name":name,"role":label,
            "x":pos.0,"z":pos.1,"width":width,"depth":depth,"yaw":yaw,"radius":radius,"fields":all}));
    };
    for (i, body) in scene.scene.engine.bodies.iter().enumerate() {
        let component = if scene.scene.doors.iter().any(|d| d.body == i) { 1.0 } else if scene.scene.chests.iter().any(|c| c.body == i) { 2.0 } else { 0.0 };
        let role = if component == 1.0 { "Door" } else if component == 2.0 { "Chest" }
            else if body.what.fixed() && body.what.interactable() { "Switch" }
            else if body.what.fixed() { "Prop" } else { "Enemy" };
        let mut fields = template_fields(body.what);
        let mut choice = field("component", "Role", "choice", component, 1.0);
        choice["options"] = json!(["Body", "Door", "Chest"]);
        fields.push(choice);
        fields.push(field("attacker", "Contact attacker (one per scene)", "bool", f32::from(scene.scene.enemy_attack.is_some_and(|a| a.body == i)), 1.0));
        push(Kind::Body, i, role, body.pos, 0.0, 0.0, 0.0, arpg_sim::ENEMY_RADIUS, fields);
    }
    for (i, wall) in scene.scene.engine.walls.iter().enumerate() {
        push(Kind::Wall, i, "Wall", wall.centre().into(), wall.size().x, wall.size().y, wall.yaw(), 0.0,
            vec![field("width", "Width", "m", wall.size().x, 0.5), field("depth", "Thickness", "m", wall.size().y, 0.1), field("yaw", "Rotation", "°", wall.yaw().to_degrees(), 15.0)]);
    }
    for (i, source) in scene.scene.engine.sources.iter().enumerate() {
        let mut fields = vec![field("radius", "Spawn radius", "m", source.radius, 0.5), integer_field("every", "Interval", "ticks", source.every), field("enabled", "Enabled", "bool", f32::from(source.enabled), 1.0)];
        fields.extend(template_fields(source.what));
        let condition = match source.when { Condition::Always => 0.0, Condition::FewerThan(_) => 1.0, Condition::PlayerWithin(_) => 2.0 };
        let mut choice = field("condition", "Condition", "choice", condition, 1.0);
        choice["options"] = json!(["Always", "World enemies below", "Player within"]);
        fields.push(choice);
        match source.when {
            Condition::Always => {}
            Condition::FewerThan(n) => fields.push(integer_field("threshold", "Population limit", "enemies", n)),
            Condition::PlayerWithin(r) => fields.push(field("threshold", "Trigger distance", "m", r, 0.5)),
        }
        push(Kind::Source, i, "Source", source.pos, 0.0, 0.0, 0.0, source.radius, fields);
    }
    for (i, grid) in scene.scene.engine.grids.iter().enumerate() {
        let columns = grid.columns.checked_sub(1).ok_or("Formation columns must be positive.")?;
        let rows = grid.rows.checked_sub(1).ok_or("Formation rows must be positive.")?;
        let mut fields = vec![integer_field("columns", "Columns", "", grid.columns), integer_field("rows", "Rows", "", grid.rows), field("spacing", "Spacing", "m", grid.spacing, 0.25)];
        fields.extend(template_fields(grid.what));
        push(Kind::Grid, i, "Formation", grid.origin, columns as f32 * grid.spacing, rows as f32 * grid.spacing, 0.0, arpg_sim::ENEMY_RADIUS, fields);
    }
    for (i, tree) in scene.scene.trees.iter().enumerate() {
        let mut kind = field("kind", "Variant", "choice", match tree.kind() { TreeKind::Pine => 0.0, TreeKind::Broadleaf => 1.0, TreeKind::Bare => 2.0 }, 1.0);
        kind["options"] = json!(["Pine", "Broadleaf", "Bare"]);
        let collider = tree.collider();
        push(Kind::Tree, i, "Tree", tree.position().into(), collider.size().x, collider.size().y, tree.yaw(), 1.2 * tree.scale(),
            vec![kind, field("scale", "Scale", "×", tree.scale(), 0.1), field("yaw", "Rotation", "°", tree.yaw().to_degrees(), 15.0)]);
    }
    for (i, torch) in scene.scene.torches.iter().enumerate() {
        let light = torch.light();
        push(Kind::Campfire, i, "Campfire", (light.position().x, light.position().z), 0.0, 0.0, 0.0, light.range(),
            vec![colour_field("colour", "Light colour", light.colour()), field("intensity", "Brightness", "", light.intensity(), 1.0),
                field("range", "Light range", "m", light.range(), 0.5), field("height", "Light height", "m", light.position().y, 0.1)]);
    }
    let selected = selected.map(target_id);
    let light = scene.scene.environment.light();
    let direction = light.direction().normalize();
    let lighting = vec![colour_field("ambient_colour", "Ambient colour", light.ambient_colour()),
        field("ambient_intensity", "Ambient brightness", "", light.ambient_intensity(), 0.05),
        colour_field("directional_colour", "Sun / moon colour", light.directional_colour()),
        field("directional_intensity", "Sun / moon brightness", "", light.directional_intensity(), 0.05),
        field("azimuth", "Direction", "°", direction.x.atan2(direction.z).to_degrees(), 15.0),
        field("elevation", "Elevation", "°", direction.y.clamp(-1.0, 1.0).asin().to_degrees(), 5.0)];
    let (encounters, rewards) = encounters::project(scene);
    let result = json!({"document":scene,"name":scene.scene.engine.name,"objects":objects,"selected":selected,"path":path,"revision":revision,
        "arena_half":arpg_sim::ARENA_HALF,"player_radius":arpg_sim::PLAYER_RADIUS,
        "projection":arpg_core::isometric_ground_projection(),
        "lighting":lighting,
        "encounters":encounters,"rewards":rewards});
    if result.to_string().len() > 6 * 1024 * 1024 { return Err("Scene is too large for the editor.".into()); }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_preserves_complete_scene_and_remaps_relationships_on_delete() {
        let mut scene = super::authoring_tests::composed_scene();
        // Insert an unrelated body/source before every referenced object, then
        // remove them. Every original relationship and state hash must return.
        let original = Game::from_scene(&scene.scene).unwrap().hash();
        scene.scene.engine.bodies.insert(0, Placed { pos: (70.0, 70.0), what: Template::BODY });
        for d in &mut scene.scene.doors { d.body += 1; }
        for c in &mut scene.scene.chests { c.body += 1; }
        if let Some(a) = &mut scene.scene.enemy_attack { a.body += 1; }
        for e in &mut scene.scene.encounters { let arpg_game::EncounterStart::Activated(i) = &mut e.start; *i += 1; }
        scene.scene.engine.sources.insert(0, scene.scene.engine.sources[0]);
        for e in &mut scene.scene.encounters { let arpg_game::EncounterObjective::Defeat(goal) = &mut e.objective; for i in &mut goal.sources { *i += 1; } }
        let (scene, _) = edit(scene, Edit::Delete { target: Target { kind: Kind::Body, index: 0 } }).unwrap();
        let (scene, _) = edit(scene, Edit::Delete { target: Target { kind: Kind::Source, index: 0 } }).unwrap();
        assert_eq!(Game::from_scene(&scene.scene).unwrap().hash(), original);
        let arpg_game::EncounterStart::Activated(index) = scene.scene.encounters[0].start;
        assert!(edit(scene.clone(), Edit::Delete { target: Target { kind: Kind::Body, index } }).is_err());
        let arpg_game::EncounterObjective::Defeat(goal) = &scene.scene.encounters[0].objective;
        assert!(edit(scene.clone(), Edit::Delete { target: Target { kind: Kind::Source, index: goal.sources[0] } }).is_err());
        let value = serde_json::to_value(&scene).unwrap();
        let decoded: SceneDocument = serde_json::from_value(value).unwrap();
        assert_eq!(Game::from_scene(&decoded.scene).unwrap().hash(), original);
    }

    #[test]
    fn geometry_edits_and_formations_are_validated_before_the_draft_is_returned() {
        let (scene, _) = edit(SceneDocument::default(), Edit::DrawWall { start: (-4.0, 2.0), end: (4.0, 2.5) }).unwrap();
        assert_eq!(scene.scene.engine.walls[0].size(), Vec2::new(8.0, 0.5));
        let wall = Target { kind: Kind::Wall, index: 0 };
        assert!(edit(scene.clone(), Edit::Move { target: wall, pos: (96.0, 96.0) }).is_err());
        assert!(edit(scene.clone(), Edit::Set { target: wall, field: "depth".into(), value: Number::from_f64(0.0).unwrap() }).is_err());
        let (scene, _) = edit(scene, Edit::Duplicate { target: wall, offset: (0.0, 5.0) }).unwrap();
        assert_eq!(scene.scene.engine.walls.len(), 2);
        let (scene, _) = edit(scene, Edit::Add { kind: Kind::Grid, pos: (10.0, 10.0) }).unwrap();
        assert_eq!(Game::from_scene(&scene.scene).unwrap().enemy_count(), 9);
        let grid = Target { kind: Kind::Grid, index: 0 };
        assert!(edit(scene.clone(), Edit::Set { target: grid, field: "columns".into(), value: Number::from_f64(1.5).unwrap() }).is_err());
        assert!(edit(scene.clone(), Edit::Set { target: grid, field: "rows".into(), value: Number::from_f64(0.0).unwrap() }).is_err());
        assert!(edit(scene, Edit::Move { target: grid, pos: (f32::NAN, 0.0) }).is_err());
    }

    #[test]
    fn trees_and_campfires_edit_through_the_same_scene_lifetime() {
        let (scene, _) = edit(SceneDocument::default(), Edit::Add { kind: Kind::Tree, pos: (8.0, 6.0) }).unwrap();
        let tree = Target { kind: Kind::Tree, index: 0 };
        let (scene, _) = edit(scene, Edit::Set { target: tree, field: "kind".into(), value: Number::from_f64(1.0).unwrap() }).unwrap();
        let (scene, _) = edit(scene, Edit::Set { target: tree, field: "scale".into(), value: Number::from_f64(1.5).unwrap() }).unwrap();
        let (scene, _) = edit(scene, Edit::Duplicate { target: tree, offset: (4.0, 0.0) }).unwrap();
        assert_eq!(scene.scene.trees[1].kind(), TreeKind::Broadleaf);
        assert_eq!(scene.scene.trees[1].position(), Vec2::new(12.0, 6.0));
        assert!(edit(scene.clone(), Edit::Set { target: tree, field: "scale".into(), value: Number::from_f64(4.0).unwrap() }).is_err());
        let (scene, _) = edit(scene, Edit::Add { kind: Kind::Campfire, pos: (3.0, 4.0) }).unwrap();
        let fire = Target { kind: Kind::Campfire, index: 0 };
        let (scene, _) = edit(scene, Edit::Colour { target: fire, colour: (0.1, 0.3, 1.0) }).unwrap();
        let (scene, _) = edit(scene, Edit::Move { target: fire, pos: (5.0, 2.0) }).unwrap();
        let (scene, _) = edit(scene, Edit::Set { target: fire, field: "height".into(), value: Number::from_f64(2.5).unwrap() }).unwrap();
        assert!(edit(scene.clone(), Edit::Set { target: fire, field: "height".into(), value: Number::from_f64(0.0).unwrap() }).is_err());
        assert_eq!(scene.scene.torches[0].light().colour().to_array(), [0.1, 0.3, 1.0]);
        for invalid in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(edit(scene.clone(), Edit::Colour { target: fire, colour: (invalid, 1.0, 1.0) }).is_err());
        }
        // Predict complete admission, then restart: both trunks and the moved
        // light must return with exactly the authored settings.
        let mut game = Game::from_scene(&scene.scene).unwrap();
        assert_eq!(game.trees().count(), 2);
        assert_eq!(game.torches().next().unwrap().light().position(), Vec3::new(5.0, 2.5, 2.0));
        let hash = game.hash();
        game.restart().unwrap();
        assert_eq!(game.hash(), hash);
        let document = document(&scene, Some(fire), None, None).unwrap();
        assert_eq!(document["objects"].as_array().unwrap().len(), 3);
        assert_eq!(document["selected"], "campfire:0");
        let (scene, _) = edit(scene, Edit::Delete { target: tree }).unwrap();
        let (scene, _) = edit(scene, Edit::Delete { target: fire }).unwrap();
        let game = Game::from_scene(&scene.scene).unwrap();
        assert_eq!(game.trees().count(), 1);
        assert_eq!(game.torch_count(), 0);
    }

    #[test]
    fn environment_edits_are_validated_and_independent_of_campfires() {
        let (scene, _) = edit(SceneDocument::default(), Edit::Lighting { field: "ambient_intensity".into(), value: 0.1 }).unwrap();
        let (scene, _) = edit(scene, Edit::LightingColour { field: "ambient_colour".into(), colour: (0.1, 0.2, 0.8) }).unwrap();
        let (scene, _) = edit(scene, Edit::Lighting { field: "azimuth".into(), value: 90.0 }).unwrap();
        let (scene, _) = edit(scene, Edit::Lighting { field: "elevation".into(), value: 45.0 }).unwrap();
        let expected = scene.scene.environment.light();
        assert_eq!(expected.ambient_colour().to_array(), [0.1, 0.2, 0.8]);
        for field in ["ambient_colour", "directional_colour"] {
            for invalid in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
                assert!(edit(scene.clone(), Edit::LightingColour { field: field.into(), colour: (1.0, invalid, 1.0) }).is_err());
            }
        }
        assert!((expected.direction() - Vec3::new(0.5_f32.sqrt(), 0.5_f32.sqrt(), 0.0)).length() < 0.0001);
        assert!(edit(scene.clone(), Edit::Lighting { field: "ambient_intensity".into(), value: -1.0 }).is_err());
        assert!(edit(scene.clone(), Edit::Lighting { field: "elevation".into(), value: 100.0 }).is_err());
        let (scene, _) = edit(scene, Edit::Add { kind: Kind::Campfire, pos: (3.0, 4.0) }).unwrap();
        assert_eq!(Game::from_scene(&scene.scene).unwrap().environment_light(), expected);
        let (scene, _) = edit(scene, Edit::Delete { target: Target { kind: Kind::Campfire, index: 0 } }).unwrap();
        assert_eq!(Game::from_scene(&scene.scene).unwrap().environment_light(), expected);
    }
}
