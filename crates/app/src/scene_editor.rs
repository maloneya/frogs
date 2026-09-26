//! Stateless authoring operations. The companion retains drafts/undo; each
//! request carries a complete game-owned scene, never a live-world snapshot.
use arpg_core::{EnvironmentLight, PointLight};
use arpg_game::{EnvironmentSpec, Game, GameScene, TorchSpec, TreeKind, TreeSpec};
use arpg_sim::{BodyGrid, Condition, Placed, SourceSpec, Template, Wall};
use glam::{Vec2, Vec3};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    New,
    Open { path: PathBuf },
    Edit { scene: GameScene, edit: Edit },
    Save { scene: GameScene, path: PathBuf, revision: Option<String>, play: bool },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind { Body, Wall, Source, Grid, Tree, Campfire }

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Target { pub kind: Kind, pub index: usize }

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Edit {
    Add { kind: Kind, pos: (f32, f32) },
    DrawWall { start: (f32, f32), end: (f32, f32) },
    Move { target: Target, pos: (f32, f32) },
    Set { target: Target, field: String, value: f32 },
    Colour { target: Target, colour: (f32, f32, f32) },
    Lighting { field: String, value: f32 },
    LightingColour { field: String, colour: (f32, f32, f32) },
    Duplicate { target: Target, offset: (f32, f32) },
    Delete { target: Target },
    Rename { name: String },
}

fn missing() -> String { "The selected object no longer exists.".into() }

fn position(scene: &GameScene, target: Target) -> Result<(f32, f32), String> {
    match target.kind {
        Kind::Body => scene.engine.bodies.get(target.index).map(|v| v.pos),
        Kind::Wall => scene.engine.walls.get(target.index).map(|v| v.centre().into()),
        Kind::Source => scene.engine.sources.get(target.index).map(|v| v.pos),
        Kind::Grid => scene.engine.grids.get(target.index).map(|v| v.origin),
        Kind::Tree => scene.trees.get(target.index).map(|v| v.position().into()),
        Kind::Campfire => scene.torches.get(target.index).map(|v| (v.light().position().x, v.light().position().z)),
    }.ok_or_else(missing)
}

fn move_to(scene: &mut GameScene, target: Target, pos: (f32, f32)) -> Result<(), String> {
    if !Vec2::from(pos).is_finite() { return Err("Position must be finite.".into()); }
    match target.kind {
        Kind::Body => scene.engine.bodies.get_mut(target.index).ok_or_else(missing)?.pos = pos,
        Kind::Source => scene.engine.sources.get_mut(target.index).ok_or_else(missing)?.pos = pos,
        Kind::Grid => scene.engine.grids.get_mut(target.index).ok_or_else(missing)?.origin = pos,
        Kind::Wall => {
            let wall = scene.engine.walls.get_mut(target.index).ok_or_else(missing)?;
            *wall = Wall::new(pos.into(), wall.size(), wall.yaw())?;
        }
        Kind::Tree => {
            let tree = scene.trees.get_mut(target.index).ok_or_else(missing)?;
            *tree = TreeSpec::new(tree.kind(), pos.into(), tree.yaw(), tree.scale())?;
        }
        Kind::Campfire => {
            let torch = scene.torches.get_mut(target.index).ok_or_else(missing)?;
            let light = torch.light();
            *torch = TorchSpec::new(PointLight::try_new(Vec3::new(pos.0, light.position().y, pos.1), light.colour(), light.intensity(), light.range())?)?;
        }
    }
    Ok(())
}

fn integer(value: f32) -> Result<u32, String> {
    if !(0.0..=16_777_215.0).contains(&value) || value.fract() != 0.0 {
        return Err("Enter a nonnegative whole number below 16,777,216.".into());
    }
    Ok(value as u32)
}

fn seeking(template: &mut Template, value: f32) -> Result<(), String> {
    if template.fixed() { return Err("Static props cannot seek.".into()); }
    if value != 0.0 && value != 1.0 { return Err("Seeking must be on or off.".into()); }
    let mut next = if value == 1.0 { Template::BODY.seeking() } else { Template::BODY };
    if template.interactable() { next = next.interactive(); }
    *template = next;
    Ok(())
}

fn set(scene: &mut GameScene, target: Target, field: &str, value: f32) -> Result<(), String> {
    if !value.is_finite() { return Err("Value must be finite.".into()); }
    if field == "x" || field == "z" {
        let (x, z) = position(scene, target)?;
        return move_to(scene, target, if field == "x" { (value, z) } else { (x, value) });
    }
    match target.kind {
        Kind::Body if field == "seeks" => seeking(&mut scene.engine.bodies.get_mut(target.index).ok_or_else(missing)?.what, value)?,
        Kind::Wall => {
            let wall = scene.engine.walls.get_mut(target.index).ok_or_else(missing)?;
            let mut size = wall.size();
            let mut yaw = wall.yaw();
            match field { "width" => size.x = value, "depth" => size.y = value,
                "yaw" => yaw = value.to_radians(), _ => return Err("Unknown wall field.".into()) }
            *wall = Wall::new(wall.centre(), size, yaw)?;
        }
        Kind::Source => {
            let source = scene.engine.sources.get_mut(target.index).ok_or_else(missing)?;
            match field {
                "radius" => source.radius = value,
                "every" => source.every = integer(value)?,
                "enabled" if value == 0.0 || value == 1.0 => source.enabled = value == 1.0,
                "seeks" => seeking(&mut source.what, value)?,
                "condition" => source.when = match integer(value)? {
                    0 => Condition::Always, 1 => Condition::FewerThan(12), 2 => Condition::PlayerWithin(10.0),
                    _ => return Err("Unknown source condition.".into()),
                },
                "threshold" => match &mut source.when {
                    Condition::FewerThan(n) => *n = integer(value)? as usize,
                    Condition::PlayerWithin(r) => *r = value,
                    Condition::Always => return Err("Always has no threshold.".into()),
                },
                _ => return Err("Unknown source field.".into()),
            }
        }
        Kind::Grid => {
            let grid = scene.engine.grids.get_mut(target.index).ok_or_else(missing)?;
            match field {
                "columns" => grid.columns = integer(value)? as usize,
                "rows" => grid.rows = integer(value)? as usize,
                "spacing" => grid.spacing = value,
                "seeks" => seeking(&mut grid.what, value)?,
                _ => return Err("Unknown formation field.".into()),
            }
        }
        Kind::Tree => {
            let tree = scene.trees.get_mut(target.index).ok_or_else(missing)?;
            let (mut kind, mut yaw, mut scale) = (tree.kind(), tree.yaw(), tree.scale());
            match field {
                "kind" => kind = match integer(value)? { 0 => TreeKind::Pine, 1 => TreeKind::Broadleaf, 2 => TreeKind::Bare, _ => return Err("Unknown tree variant.".into()) },
                "yaw" => yaw = value.to_radians(),
                "scale" => scale = value,
                _ => return Err("Unknown tree field.".into()),
            }
            *tree = TreeSpec::new(kind, tree.position(), yaw, scale)?;
        }
        Kind::Campfire => {
            let torch = scene.torches.get_mut(target.index).ok_or_else(missing)?;
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
        _ => return Err("Unknown object field.".into()),
    }
    Ok(())
}

fn lighting(scene: &mut GameScene, field: &str, value: f32) -> Result<(), String> {
    if !value.is_finite() { return Err("Value must be finite.".into()); }
    let light = scene.environment.light();
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
    scene.environment = EnvironmentSpec::new(EnvironmentLight::try_new(light.ambient_colour(), ambient, light.directional_colour(), directional, direction)?);
    Ok(())
}

/// Consumes a draft copy; refusal cannot partially change the client's document.
pub(crate) fn edit(mut scene: GameScene, edit: Edit) -> Result<(GameScene, Option<Target>), String> {
    let mut selected = None;
    match edit {
        Edit::Lighting { field, value } => lighting(&mut scene, &field, value)?,
        Edit::LightingColour { field, colour } => {
            let light = scene.environment.light();
            let (mut ambient, mut directional) = (light.ambient_colour(), light.directional_colour());
            match field.as_str() { "ambient_colour" => ambient = colour.into(), "directional_colour" => directional = colour.into(), _ => return Err("Unknown lighting colour.".into()) }
            scene.environment = EnvironmentSpec::new(EnvironmentLight::try_new(ambient, light.ambient_intensity(), directional, light.directional_intensity(), light.direction())?);
        }
        Edit::Rename { name } => {
            if name.trim().is_empty() || name.len() > 128 { return Err("Scene name must contain 1–128 bytes.".into()); }
            scene.engine.name = name;
        }
        Edit::Add { kind, pos } => {
            let index = match kind {
                Kind::Body => { scene.engine.bodies.push(Placed { pos, what: Template::BODY.seeking() }); scene.engine.bodies.len() - 1 }
                Kind::Wall => { scene.engine.walls.push(Wall::new(pos.into(), Vec2::new(4.0, 0.4), 0.0)?); scene.engine.walls.len() - 1 }
                Kind::Source => { scene.engine.sources.push(SourceSpec { pos, radius: 2.0, every: 60, enabled: true, when: Condition::FewerThan(12), what: Template::BODY.seeking() }); scene.engine.sources.len() - 1 }
                Kind::Grid => { scene.engine.grids.push(BodyGrid { origin: pos, columns: 3, rows: 3, spacing: 1.5, what: Template::BODY.seeking() }); scene.engine.grids.len() - 1 }
                Kind::Tree => { scene.trees.push(TreeSpec::new(TreeKind::Pine, pos.into(), 0.0, 1.0)?); scene.trees.len() - 1 }
                Kind::Campfire => { scene.torches.push(TorchSpec::new(PointLight::try_new(Vec3::new(pos.0, 1.5, pos.1), Vec3::new(1.0, 0.45, 0.12), 24.0, 14.0)?)?); scene.torches.len() - 1 }
            };
            selected = Some(Target { kind, index });
        }
        Edit::DrawWall { start, end } => {
            let start = Vec2::from(start);
            let end = Vec2::from(end);
            let wall = Wall::new((start + end) * 0.5, (end - start).abs().max(Vec2::splat(0.1)), 0.0)?;
            scene.engine.walls.push(wall);
            selected = Some(Target { kind: Kind::Wall, index: scene.engine.walls.len() - 1 });
        }
        Edit::Move { target, pos } => { move_to(&mut scene, target, pos)?; selected = Some(target); }
        Edit::Set { target, field, value } => { set(&mut scene, target, &field, value)?; selected = Some(target); }
        Edit::Colour { target, colour } => {
            if !matches!(target.kind, Kind::Campfire) { return Err("This object has no light colour.".into()); }
            let torch = scene.torches.get_mut(target.index).ok_or_else(missing)?;
            let light = torch.light();
            *torch = TorchSpec::new(PointLight::try_new(light.position(), colour.into(), light.intensity(), light.range())?)?;
            selected = Some(target);
        }
        Edit::Duplicate { target, offset } => {
            let (x, z) = position(&scene, target)?;
            let index = match target.kind {
                Kind::Body => { scene.engine.bodies.push(scene.engine.bodies[target.index]); scene.engine.bodies.len() - 1 }
                Kind::Wall => { scene.engine.walls.push(scene.engine.walls[target.index]); scene.engine.walls.len() - 1 }
                Kind::Source => { scene.engine.sources.push(scene.engine.sources[target.index]); scene.engine.sources.len() - 1 }
                Kind::Grid => { scene.engine.grids.push(scene.engine.grids[target.index]); scene.engine.grids.len() - 1 }
                Kind::Tree => { scene.trees.push(scene.trees[target.index]); scene.trees.len() - 1 }
                Kind::Campfire => { scene.torches.push(scene.torches[target.index]); scene.torches.len() - 1 }
            };
            let next = Target { kind: target.kind, index };
            move_to(&mut scene, next, (x + offset.0, z + offset.1))?;
            selected = Some(next);
        }
        Edit::Delete { target } => {
            position(&scene, target)?;
            match target.kind {
                Kind::Body => scene.remove_body(target.index)?,
                Kind::Source => scene.remove_source(target.index)?,
                Kind::Wall => { scene.engine.walls.remove(target.index); }
                Kind::Grid => { scene.engine.grids.remove(target.index); }
                Kind::Tree => { scene.trees.remove(target.index); }
                Kind::Campfire => { scene.torches.remove(target.index); }
            }
        }
    }
    Game::from_scene(&scene).map_err(|error| error.to_string())?;
    Ok((scene, selected))
}

fn field(key: &str, label: &str, unit: &str, value: f32, step: f32) -> Value {
    json!({"key":key,"label":label,"unit":unit,"value":value,"step":step})
}

fn colour_field(key: &str, label: &str, colour: Vec3) -> Value {
    json!({"key":key,"label":label,"unit":"colour","value":0.0,"step":0.0,"colour":colour.to_array()})
}

/// Drawing and inspector data are derived from the authoritative description;
/// Swift carries the opaque scene back without reconstructing its schema.
pub(crate) fn document(scene: &GameScene, selected: Option<Target>, path: Option<PathBuf>, revision: Option<String>) -> Result<Value, String> {
    let count = scene.engine.bodies.len() + scene.engine.walls.len() + scene.engine.sources.len() + scene.engine.grids.len() + scene.trees.len() + scene.torches.len();
    if count > 4096 { return Err("The editor supports 4096 placements; use formations for larger crowds.".into()); }
    let mut objects = Vec::new();
    let mut push = |kind: &str, index: usize, label: &str, pos: (f32, f32), width: f32, depth: f32, yaw: f32, radius: f32, mut fields: Vec<Value>| {
        let mut all = vec![field("x", "X", "m", pos.0, 0.5), field("z", "Z", "m", pos.1, 0.5)];
        all.append(&mut fields);
        objects.push(json!({"id":format!("{kind}:{index}"),"kind":kind,"index":index,"label":format!("{label} {}", index + 1),
            "x":pos.0,"z":pos.1,"width":width,"depth":depth,"yaw":yaw,"radius":radius,"fields":all}));
    };
    for (i, body) in scene.engine.bodies.iter().enumerate() {
        let fields = if body.what.fixed() { vec![] } else { vec![field("seeks", "Seeks player", "bool", f32::from(body.what.seeks()), 1.0)] };
        push("body", i, if body.what.fixed() { "Prop" } else { "Enemy" }, body.pos, 0.0, 0.0, 0.0, arpg_sim::ENEMY_RADIUS, fields);
    }
    for (i, wall) in scene.engine.walls.iter().enumerate() {
        push("wall", i, "Wall", wall.centre().into(), wall.size().x, wall.size().y, wall.yaw(), 0.0,
            vec![field("width", "Width", "m", wall.size().x, 0.5), field("depth", "Thickness", "m", wall.size().y, 0.1), field("yaw", "Rotation", "°", wall.yaw().to_degrees(), 15.0)]);
    }
    for (i, source) in scene.engine.sources.iter().enumerate() {
        let mut fields = vec![field("radius", "Spawn radius", "m", source.radius, 0.5), field("every", "Interval", "ticks", source.every as f32, 1.0), field("enabled", "Enabled", "bool", f32::from(source.enabled), 1.0)];
        if !source.what.fixed() { fields.push(field("seeks", "Seeks player", "bool", f32::from(source.what.seeks()), 1.0)); }
        let condition = match source.when { Condition::Always => 0.0, Condition::FewerThan(_) => 1.0, Condition::PlayerWithin(_) => 2.0 };
        let mut choice = field("condition", "Condition", "choice", condition, 1.0);
        choice["options"] = json!(["Always", "World enemies below", "Player within"]);
        fields.push(choice);
        match source.when {
            Condition::Always => {}
            Condition::FewerThan(n) => fields.push(field("threshold", "Population limit", "enemies", n as f32, 1.0)),
            Condition::PlayerWithin(r) => fields.push(field("threshold", "Trigger distance", "m", r, 0.5)),
        }
        push("source", i, "Source", source.pos, 0.0, 0.0, 0.0, source.radius, fields);
    }
    for (i, grid) in scene.engine.grids.iter().enumerate() {
        // Save requests reach projection before full scene admission. Refuse
        // malformed dimensions rather than underflowing while deriving extents.
        let columns = grid.columns.checked_sub(1).ok_or("Formation columns must be positive.")?;
        let rows = grid.rows.checked_sub(1).ok_or("Formation rows must be positive.")?;
        let mut fields = vec![field("columns", "Columns", "", grid.columns as f32, 1.0), field("rows", "Rows", "", grid.rows as f32, 1.0), field("spacing", "Spacing", "m", grid.spacing, 0.25)];
        if !grid.what.fixed() { fields.push(field("seeks", "Seeks player", "bool", f32::from(grid.what.seeks()), 1.0)); }
        push("grid", i, "Formation", grid.origin, columns as f32 * grid.spacing, rows as f32 * grid.spacing, 0.0, arpg_sim::ENEMY_RADIUS, fields);
    }
    for (i, tree) in scene.trees.iter().enumerate() {
        let mut kind = field("kind", "Variant", "choice", match tree.kind() { TreeKind::Pine => 0.0, TreeKind::Broadleaf => 1.0, TreeKind::Bare => 2.0 }, 1.0);
        kind["options"] = json!(["Pine", "Broadleaf", "Bare"]);
        let collider = tree.collider();
        push("tree", i, "Tree", tree.position().into(), collider.size().x, collider.size().y, tree.yaw(), 1.2 * tree.scale(),
            vec![kind, field("scale", "Scale", "×", tree.scale(), 0.1), field("yaw", "Rotation", "°", tree.yaw().to_degrees(), 15.0)]);
    }
    for (i, torch) in scene.torches.iter().enumerate() {
        let light = torch.light();
        push("campfire", i, "Campfire", (light.position().x, light.position().z), 0.0, 0.0, 0.0, light.range(),
            vec![colour_field("colour", "Light colour", light.colour()), field("intensity", "Brightness", "", light.intensity(), 1.0),
                field("range", "Light range", "m", light.range(), 0.5), field("height", "Light height", "m", light.position().y, 0.1)]);
    }
    let selected = selected.map(|s| format!("{}:{}", match s.kind { Kind::Body => "body", Kind::Wall => "wall", Kind::Source => "source", Kind::Grid => "grid", Kind::Tree => "tree", Kind::Campfire => "campfire" }, s.index));
    let light = scene.environment.light();
    let direction = light.direction().normalize();
    let lighting = vec![colour_field("ambient_colour", "Ambient colour", light.ambient_colour()),
        field("ambient_intensity", "Ambient brightness", "", light.ambient_intensity(), 0.05),
        colour_field("directional_colour", "Sun / moon colour", light.directional_colour()),
        field("directional_intensity", "Sun / moon brightness", "", light.directional_intensity(), 0.05),
        field("azimuth", "Direction", "°", direction.x.atan2(direction.z).to_degrees(), 15.0),
        field("elevation", "Elevation", "°", direction.y.clamp(-1.0, 1.0).asin().to_degrees(), 5.0)];
    let result = json!({"scene":scene,"name":scene.engine.name,"objects":objects,"selected":selected,"path":path,"revision":revision,
        "arena_half":arpg_sim::ARENA_HALF,"player_radius":arpg_sim::PLAYER_RADIUS,
        "projection":arpg_gfx::OrthoCamera::new(1, 1).ground_projection(),
        "lighting":lighting,
        "relationships":scene.encounters.len() + scene.doors.len() + scene.chests.len() + usize::from(scene.enemy_attack.is_some())});
    if result.to_string().len() > 6 * 1024 * 1024 { return Err("Scene is too large for the editor.".into()); }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_preserves_complete_scene_and_remaps_relationships_on_delete() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes/soul_trial.ron");
        let mut scene = arpg_content::load_scene(&path).unwrap();
        // Insert an unrelated body/source before every referenced object, then
        // remove them. Every original relationship and state hash must return.
        let original = Game::from_scene(&scene).unwrap().hash();
        scene.engine.bodies.insert(0, Placed { pos: (70.0, 70.0), what: Template::BODY });
        for d in &mut scene.doors { d.body += 1; }
        for c in &mut scene.chests { c.body += 1; }
        if let Some(a) = &mut scene.enemy_attack { a.body += 1; }
        for e in &mut scene.encounters { let arpg_game::EncounterStart::Activated(i) = &mut e.start; *i += 1; }
        scene.engine.sources.insert(0, scene.engine.sources[0]);
        for e in &mut scene.encounters { let arpg_game::EncounterObjective::Defeat(goal) = &mut e.objective; for i in &mut goal.sources { *i += 1; } }
        let (scene, _) = edit(scene, Edit::Delete { target: Target { kind: Kind::Body, index: 0 } }).unwrap();
        let (scene, _) = edit(scene, Edit::Delete { target: Target { kind: Kind::Source, index: 0 } }).unwrap();
        assert_eq!(Game::from_scene(&scene).unwrap().hash(), original);
        let arpg_game::EncounterStart::Activated(index) = scene.encounters[0].start;
        assert!(edit(scene.clone(), Edit::Delete { target: Target { kind: Kind::Body, index } }).is_err());
        let arpg_game::EncounterObjective::Defeat(goal) = &scene.encounters[0].objective;
        assert!(edit(scene.clone(), Edit::Delete { target: Target { kind: Kind::Source, index: goal.sources[0] } }).is_err());
        let value = serde_json::to_value(&scene).unwrap();
        let decoded: GameScene = serde_json::from_value(value).unwrap();
        assert_eq!(Game::from_scene(&decoded).unwrap().hash(), original);
    }

    #[test]
    fn geometry_edits_and_formations_are_validated_before_the_draft_is_returned() {
        let (scene, _) = edit(GameScene::default(), Edit::DrawWall { start: (-4.0, 2.0), end: (4.0, 2.5) }).unwrap();
        assert_eq!(scene.engine.walls[0].size(), Vec2::new(8.0, 0.5));
        let wall = Target { kind: Kind::Wall, index: 0 };
        assert!(edit(scene.clone(), Edit::Move { target: wall, pos: (96.0, 96.0) }).is_err());
        assert!(edit(scene.clone(), Edit::Set { target: wall, field: "depth".into(), value: 0.0 }).is_err());
        let (scene, _) = edit(scene, Edit::Duplicate { target: wall, offset: (0.0, 5.0) }).unwrap();
        assert_eq!(scene.engine.walls.len(), 2);
        let (scene, _) = edit(scene, Edit::Add { kind: Kind::Grid, pos: (10.0, 10.0) }).unwrap();
        assert_eq!(Game::from_scene(&scene).unwrap().enemy_count(), 9);
        let grid = Target { kind: Kind::Grid, index: 0 };
        assert!(edit(scene.clone(), Edit::Set { target: grid, field: "columns".into(), value: 1.5 }).is_err());
        assert!(edit(scene.clone(), Edit::Set { target: grid, field: "rows".into(), value: 0.0 }).is_err());
        assert!(edit(scene, Edit::Move { target: grid, pos: (f32::NAN, 0.0) }).is_err());
    }

    #[test]
    fn trees_and_campfires_edit_through_the_same_scene_lifetime() {
        let (scene, _) = edit(GameScene::default(), Edit::Add { kind: Kind::Tree, pos: (8.0, 6.0) }).unwrap();
        let tree = Target { kind: Kind::Tree, index: 0 };
        let (scene, _) = edit(scene, Edit::Set { target: tree, field: "kind".into(), value: 1.0 }).unwrap();
        let (scene, _) = edit(scene, Edit::Set { target: tree, field: "scale".into(), value: 1.5 }).unwrap();
        let (scene, _) = edit(scene, Edit::Duplicate { target: tree, offset: (4.0, 0.0) }).unwrap();
        assert_eq!(scene.trees[1].kind(), TreeKind::Broadleaf);
        assert_eq!(scene.trees[1].position(), Vec2::new(12.0, 6.0));
        assert!(edit(scene.clone(), Edit::Set { target: tree, field: "scale".into(), value: 4.0 }).is_err());
        let (scene, _) = edit(scene, Edit::Add { kind: Kind::Campfire, pos: (3.0, 4.0) }).unwrap();
        let fire = Target { kind: Kind::Campfire, index: 0 };
        let (scene, _) = edit(scene, Edit::Colour { target: fire, colour: (0.1, 0.3, 1.0) }).unwrap();
        let (scene, _) = edit(scene, Edit::Move { target: fire, pos: (5.0, 2.0) }).unwrap();
        let (scene, _) = edit(scene, Edit::Set { target: fire, field: "height".into(), value: 2.5 }).unwrap();
        assert!(edit(scene.clone(), Edit::Set { target: fire, field: "height".into(), value: 0.0 }).is_err());
        assert!(edit(scene.clone(), Edit::Colour { target: fire, colour: (-0.1, 1.0, 1.0) }).is_err());
        // Predict complete admission, then restart: both trunks and the moved
        // light must return with exactly the authored settings.
        let mut game = Game::from_scene(&scene).unwrap();
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
        let game = Game::from_scene(&scene).unwrap();
        assert_eq!(game.trees().count(), 1);
        assert_eq!(game.torch_count(), 0);
    }

    #[test]
    fn environment_edits_are_validated_and_independent_of_campfires() {
        let (scene, _) = edit(GameScene::default(), Edit::Lighting { field: "ambient_intensity".into(), value: 0.1 }).unwrap();
        let (scene, _) = edit(scene, Edit::LightingColour { field: "ambient_colour".into(), colour: (0.1, 0.2, 0.8) }).unwrap();
        let (scene, _) = edit(scene, Edit::Lighting { field: "azimuth".into(), value: 90.0 }).unwrap();
        let (scene, _) = edit(scene, Edit::Lighting { field: "elevation".into(), value: 45.0 }).unwrap();
        let expected = scene.environment.light();
        assert!((expected.direction() - Vec3::new(0.5_f32.sqrt(), 0.5_f32.sqrt(), 0.0)).length() < 0.0001);
        assert!(edit(scene.clone(), Edit::Lighting { field: "ambient_intensity".into(), value: -1.0 }).is_err());
        assert!(edit(scene.clone(), Edit::Lighting { field: "elevation".into(), value: 100.0 }).is_err());
        let (scene, _) = edit(scene, Edit::Add { kind: Kind::Campfire, pos: (3.0, 4.0) }).unwrap();
        assert_eq!(Game::from_scene(&scene).unwrap().environment_light(), expected);
        let (scene, _) = edit(scene, Edit::Delete { target: Target { kind: Kind::Campfire, index: 0 } }).unwrap();
        assert_eq!(Game::from_scene(&scene).unwrap().environment_light(), expected);
    }
}
