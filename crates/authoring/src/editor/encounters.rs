//! Complete relationship edits: incomplete forms never enter the accepted draft.
use super::*;
use arpg_game::{ChestSpec, DefeatObjective, DoorSpec, EncounterAction, EncounterObjective, EncounterSpec, EncounterStart};
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Gameplay presets composed from existing body templates and components.
pub enum PropKind {
    /// Fixed non-interactive body.
    Prop,
    /// Fixed activation body.
    Switch,
    /// Locked door component on a fixed body.
    Door,
    /// Locked chest component on a fixed body.
    Chest,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Complete encounter transaction; selected sources become initially disabled.
pub struct EncounterDraft {
    /// Unique encounter name.
    pub name: String,
    /// Explicit interactable body index.
    pub trigger: usize,
    /// Ordered source indices.
    pub sources: Vec<usize>,
    /// Required defeats.
    pub count: NonZeroU32,
    /// Positive deadline in simulation ticks.
    pub within_ticks: NonZeroU32,
    /// Actions when activated.
    pub on_start: Vec<EncounterAction>,
    /// Actions on victory.
    pub on_success: Vec<EncounterAction>,
    /// Actions on timeout.
    pub on_failure: Vec<EncounterAction>,
}

pub(super) fn place(scene: &mut SceneDocument, kind: PropKind, pos: (f32, f32)) -> Target {
    let index = scene.scene.engine.bodies.len();
    let what = if matches!(kind, PropKind::Switch) { Template::BLOCK.interactive() } else { Template::BLOCK };
    scene.scene.engine.bodies.push(Placed { pos, what });
    match kind {
        PropKind::Door => scene.scene.doors.push(DoorSpec { body: index }),
        PropKind::Chest => scene.scene.chests.push(ChestSpec { body: index }),
        PropKind::Prop | PropKind::Switch => {}
    }
    Target { kind: Kind::Body, index }
}

pub(super) fn component(scene: &mut SceneDocument, body: usize, role: u32) -> Result<(), String> {
    if body >= scene.scene.engine.bodies.len() { return Err(missing()); }
    if role > 2 { return Err("Unknown body role.".into()); }
    let door = scene.scene.doors.iter().position(|d| d.body == body);
    let chest = scene.scene.chests.iter().position(|c| c.body == body);
    if (role == 1 && door.is_some()) || (role == 2 && chest.is_some()) { return Ok(()); }
    if let Some(index) = door { scene.scene.remove_door(index)?; }
    if let Some(index) = chest { scene.scene.remove_chest(index)?; }
    if role != 0 {
        scene.scene.engine.bodies[body].what = Template::BLOCK;
        if role == 1 { scene.scene.doors.push(DoorSpec { body }); }
        else { scene.scene.chests.push(ChestSpec { body }); }
    }
    Ok(())
}

pub(super) fn apply(scene: &mut SceneDocument, index: Option<usize>, draft: EncounterDraft) -> Result<(), String> {
    // Disable the complete selected group as part of the same transaction.
    // Other ownership/template errors remain the gameplay owner's decision.
    for &source in &draft.sources {
        scene.scene.engine.sources.get_mut(source).ok_or("Encounter source no longer exists.")?.enabled = false;
    }
    let spec = EncounterSpec { name: draft.name, start: EncounterStart::Activated(draft.trigger),
        objective: EncounterObjective::Defeat(DefeatObjective { sources: draft.sources, count: draft.count, within_ticks: draft.within_ticks }),
        on_start: draft.on_start, on_success: draft.on_success, on_failure: draft.on_failure };
    if let Some(index) = index { *scene.scene.encounters.get_mut(index).ok_or("Encounter no longer exists.")? = spec; }
    else { scene.scene.encounters.push(spec); }
    Ok(())
}

pub(super) fn project(scene: &SceneDocument) -> (Value, Value) {
    let encounters: Vec<_> = scene.scene.encounters.iter().enumerate().map(|(index, e)| {
        let EncounterStart::Activated(trigger) = e.start;
        let EncounterObjective::Defeat(goal) = &e.objective;
        json!({"index":index,"name":e.name,"trigger":trigger,"sources":goal.sources,
            "count":goal.count,"within_ticks":goal.within_ticks,
            "on_start":e.on_start,"on_success":e.on_success,"on_failure":e.on_failure})
    }).collect();
    let rewards: Vec<_> = scene.scene.doors.iter().enumerate().map(|(index, d)| (d.body, "Door", EncounterAction::UnlockDoor(index)))
        .chain(scene.scene.chests.iter().enumerate().map(|(index, c)| (c.body, "Chest", EncounterAction::UnlockChest(index))))
        .map(|(body, kind, action)| {
            let target = Target { kind: Kind::Body, index: body };
            let fallback = format!("{kind} {}", body + 1);
            json!({"body":body,"label":scene.object_name(target).unwrap_or(&fallback),"action":action})
        }).collect();
    (json!(encounters), json!(rewards))
}
