//! Inspector projection over the game-owned definition; admission stays in Game.
use super::{field, integer, integer_field};
use arpg_game::{
    ActionDefinition, ActorDefinition, ActorRole, BehaviorNode, Destination, TargetSelection,
};
use serde_json::{Number, Value, json};

fn choice(key: &str, label: &str, value: usize, options: &[&str]) -> Value {
    let mut result = integer_field(key, label, "choice", value);
    result["options"] = json!(options);
    result
}

fn target_index(target: TargetSelection) -> usize {
    match target {
        TargetSelection::Player => 0,
        TargetSelection::NearestHostile => 1,
        TargetSelection::NearestInteraction => 2,
    }
}
fn target(value: &Number) -> Result<TargetSelection, String> {
    match integer::<usize>(value)? {
        0 => Ok(TargetSelection::Player),
        1 => Ok(TargetSelection::NearestHostile),
        2 => Ok(TargetSelection::NearestInteraction),
        _ => Err("Unknown target selection.".into()),
    }
}
fn bool_value(value: &Number) -> Result<bool, String> {
    match integer::<usize>(value)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("Setting must be on or off.".into()),
    }
}

pub(super) fn fields(actor: &ActorDefinition) -> Vec<Value> {
    let mut out = vec![
        choice(
            "actor_role",
            "Actor role",
            match actor.role {
                ActorRole::Hostile => 0,
                ActorRole::Civilian => 1,
                ActorRole::Prop => 2,
            },
            &["Hostile", "Civilian", "Prop"],
        ),
        field(
            "fixed",
            "Fixed body",
            "bool",
            f32::from(actor.physical.fixed()),
            1.0,
        ),
        field("mass", "Mass", "kg", actor.physical.mass().kilograms(), 0.1),
        field(
            "character",
            "Character appearance",
            "bool",
            f32::from(actor.physical.is_character()),
            1.0,
        ),
        field(
            "interactable",
            "Can interact",
            "bool",
            f32::from(actor.physical.interactable()),
            1.0,
        ),
        integer_field(
            "health",
            "Health (0 = immune)",
            "",
            actor
                .physical
                .hit_points()
                .map_or(0, std::num::NonZeroU8::get),
        ),
        integer_field("action_count", "Available actions", "", actor.actions.len()),
    ];
    for (index, action) in actor.actions.iter().enumerate() {
        let prefix = format!("action.{index}");
        let label = format!("Action {}", index + 1);
        let kind = match action {
            ActionDefinition::Move(_) => 0,
            ActionDefinition::Wait { .. } => 1,
            ActionDefinition::Interact { .. } => 2,
            ActionDefinition::Swipe(_) => 3,
            ActionDefinition::Leap(_) => 4,
        };
        out.push(choice(
            &format!("{prefix}.kind"),
            &label,
            kind,
            &["Move", "Wait", "Interact", "Swipe", "Leap"],
        ));
        out.push(integer_field(
            &format!("{prefix}.order"),
            &format!("{label} · list position"),
            "",
            index + 1,
        ));
        let projected = serde_json::to_value(action).expect("finite admitted action");
        let (_, settings) = projected
            .as_object()
            .expect("action variant")
            .iter()
            .next()
            .expect("one variant");
        project_settings(settings, &prefix, &label, &mut out);
    }
    node_fields(&actor.behavior, "behavior", "Behavior", &mut out);
    out
}

fn project_settings(settings: &Value, prefix: &str, label: &str, out: &mut Vec<Value>) {
    for (name, value) in settings.as_object().expect("action settings") {
        let key = format!("{prefix}.{name}");
        let label = format!("{label} · {}", name.replace('_', " "));
        if name == "target" {
            let selection: TargetSelection =
                serde_json::from_value(value.clone()).expect("owned target");
            out.push(choice(
                &key,
                &label,
                target_index(selection),
                &["Player", "Nearest hostile", "Nearest interaction"],
            ));
        } else if name == "destination" {
            let destination: Destination =
                serde_json::from_value(value.clone()).expect("owned destination");
            let selected = match destination {
                Destination::Target(t) => target_index(t),
                Destination::Point(_) => 3,
            };
            out.push(choice(
                &key,
                &label,
                selected,
                &["Player", "Nearest hostile", "Nearest interaction", "Point"],
            ));
            if let Destination::Point((x, z)) = destination {
                out.push(field(
                    &format!("{prefix}.x"),
                    &format!("{label} X"),
                    "m",
                    x,
                    0.5,
                ));
                out.push(field(
                    &format!("{prefix}.z"),
                    &format!("{label} Z"),
                    "m",
                    z,
                    0.5,
                ));
            }
        } else if value.is_object() {
            project_settings(value, &key, &label, out);
        } else if let Some(n) = value.as_u64() {
            out.push(integer_field(&key, &label, "", n));
        } else if let Some(n) = value.as_f64() {
            out.push(field(&key, &label, "", n as f32, 0.1));
        }
    }
}

fn node_fields(node: &BehaviorNode, path: &str, label: &str, out: &mut Vec<Value>) {
    let kind = match node {
        BehaviorNode::Idle => 0,
        BehaviorNode::Action(_) => 1,
        BehaviorNode::Sequence(_) => 2,
        BehaviorNode::Priority(_) => 3,
    };
    out.push(choice(
        &format!("{path}.kind"),
        label,
        kind,
        &["Idle", "Action", "Sequence", "Priority"],
    ));
    match node {
        BehaviorNode::Action(index) => out.push(integer_field(
            &format!("{path}.action"),
            &format!("{label} · action number"),
            "",
            *index + 1,
        )),
        BehaviorNode::Sequence(children) | BehaviorNode::Priority(children) => {
            out.push(integer_field(
                &format!("{path}.children"),
                &format!("{label} · steps"),
                "",
                children.len(),
            ));
            for (index, child) in children.iter().enumerate() {
                node_fields(
                    child,
                    &format!("{path}.{index}"),
                    &format!("{label} / {}", index + 1),
                    out,
                );
            }
        }
        BehaviorNode::Idle => {}
    }
}

pub(super) fn set(actor: &mut ActorDefinition, key: &str, value: &Number) -> Result<(), String> {
    match key {
        "actor_role" => {
            actor.role = match integer::<usize>(value)? {
                0 => ActorRole::Hostile,
                1 => ActorRole::Civilian,
                2 => ActorRole::Prop,
                _ => return Err("Unknown actor role.".into()),
            }
        }
        "mass" => {
            let mass = value.as_f64().ok_or("Mass must be numeric.")? as f32;
            actor.physical = actor.physical.with_mass(arpg_sim::Mass::try_from(mass)?);
        }
        "fixed" | "character" | "interactable" | "health" => {
            let physical = actor.physical;
            let fixed = if key == "fixed" {
                bool_value(value)?
            } else {
                physical.fixed()
            };
            let character = if key == "character" {
                bool_value(value)?
            } else {
                physical.is_character()
            };
            let interactive = if key == "interactable" {
                bool_value(value)?
            } else {
                physical.interactable()
            };
            let hp = if key == "health" {
                std::num::NonZeroU8::new(integer(value)?)
            } else {
                physical.hit_points()
            };
            let mut next = if fixed {
                arpg_sim::Template::BLOCK
            } else {
                arpg_sim::Template::BODY
            };
            if character {
                next = next.character();
            }
            if interactive {
                next = next.interactive();
            }
            if let Some(hp) = hp {
                next = next.with_health(hp);
            }
            actor.physical = next.with_mass(physical.mass());
        }
        "action_count" => {
            let count = integer::<usize>(value)?;
            if count > arpg_game::MAX_ACTIONS {
                return Err("Too many actions.".into());
            }
            actor
                .actions
                .resize(count, ActionDefinition::Wait { ticks: 60 });
        }
        _ if key.starts_with("action.") => edit_action(actor, &key[7..], value)?,
        _ if key.starts_with("behavior.") => edit_node(
            &mut actor.behavior,
            &key[9..],
            value,
            !actor.actions.is_empty(),
        )?,
        _ => return Err("Unknown actor field.".into()),
    }
    actor.validate().map_err(str::to_string)
}

fn edit_action(actor: &mut ActorDefinition, key: &str, value: &Number) -> Result<(), String> {
    let (index, field) = key.split_once('.').ok_or("Missing action field.")?;
    let index = index
        .parse::<usize>()
        .map_err(|_| "Invalid action index.")?;
    if field == "order" {
        let destination = integer::<usize>(value)?
            .checked_sub(1)
            .ok_or("Action positions start at one.")?;
        if index >= actor.actions.len() || destination >= actor.actions.len() {
            return Err("Action position is outside the list.".into());
        }
        let action = actor.actions.remove(index);
        actor.actions.insert(destination, action);
        remap(&mut actor.behavior, index, destination);
        return Ok(());
    }
    let action = actor
        .actions
        .get_mut(index)
        .ok_or("Action no longer exists.")?;
    if field == "kind" {
        *action = match integer::<usize>(value)? {
            0 => ActionDefinition::Move(arpg_game::MoveAction {
                destination: Destination::Point((0.0, 0.0)),
                ..Default::default()
            }),
            1 => ActionDefinition::Wait { ticks: 60 },
            2 => ActionDefinition::Interact {
                target: TargetSelection::NearestInteraction,
            },
            3 => ActionDefinition::Swipe(arpg_game::SwipeAction {
                target: if actor.role == ActorRole::Hostile {
                    TargetSelection::Player
                } else {
                    TargetSelection::NearestHostile
                },
                ..Default::default()
            }),
            4 => ActionDefinition::Leap(arpg_game::LeapAction {
                target: if actor.role == ActorRole::Hostile {
                    TargetSelection::Player
                } else {
                    TargetSelection::NearestHostile
                },
                ..Default::default()
            }),
            _ => return Err("Unknown action kind.".into()),
        };
        return Ok(());
    }
    let mut json = serde_json::to_value(&*action).map_err(|e| e.to_string())?;
    let settings = json
        .as_object_mut()
        .expect("typed variant")
        .values_mut()
        .next()
        .expect("one variant");
    match field {
        "target" => {
            *settings
                .get_mut("target")
                .ok_or("This action has no target setting.")? =
                serde_json::to_value(target(value)?).expect("target serializes")
        }
        "destination" => {
            let destination = if integer::<usize>(value)? == 3 {
                Destination::Point((0.0, 0.0))
            } else {
                Destination::Target(target(value)?)
            };
            *settings
                .get_mut("destination")
                .ok_or("This action has no destination setting.")? =
                serde_json::to_value(destination).expect("destination serializes");
        }
        "x" | "z" => {
            let point = settings
                .get_mut("destination")
                .and_then(|v| v.get_mut("Point"))
                .and_then(Value::as_array_mut)
                .ok_or("Movement does not use a point.")?;
            point[usize::from(field == "z")] = Value::Number(value.clone());
        }
        _ => {
            let mut slot = settings;
            for part in field.split('.') {
                slot = slot.get_mut(part).ok_or("Unknown action setting.")?;
            }
            *slot = if slot.as_u64().is_some() {
                json!(integer::<u64>(value)?)
            } else {
                Value::Number(value.clone())
            };
        }
    }
    *action = serde_json::from_value(json).map_err(|e| e.to_string())?;
    Ok(())
}

fn edit_node(
    node: &mut BehaviorNode,
    key: &str,
    value: &Number,
    has_actions: bool,
) -> Result<(), String> {
    if let Some((index, tail)) = key.split_once('.') {
        let children = match node {
            BehaviorNode::Sequence(c) | BehaviorNode::Priority(c) => c,
            _ => return Err("This behavior has no children.".into()),
        };
        return edit_node(
            children
                .get_mut(
                    index
                        .parse::<usize>()
                        .map_err(|_| "Invalid behavior path.")?,
                )
                .ok_or("Behavior step no longer exists.")?,
            tail,
            value,
            has_actions,
        );
    }
    match key {
        "kind" => {
            *node = match integer::<usize>(value)? {
                0 => BehaviorNode::Idle,
                1 if has_actions => BehaviorNode::Action(0),
                2 => BehaviorNode::Sequence(vec![BehaviorNode::Idle]),
                3 => BehaviorNode::Priority(vec![BehaviorNode::Idle]),
                _ => return Err("Add an action before referencing it.".into()),
            }
        }
        "action" => match node {
            BehaviorNode::Action(index) => {
                *index = integer::<usize>(value)?
                    .checked_sub(1)
                    .ok_or("Action numbers start at one.")?
            }
            _ => return Err("Not an action step.".into()),
        },
        "children" => {
            let count = integer::<usize>(value)?;
            if count == 0 || count > arpg_game::MAX_BEHAVIOR_NODES {
                return Err("Invalid number of behavior steps.".into());
            }
            match node {
                BehaviorNode::Sequence(c) | BehaviorNode::Priority(c) => {
                    c.resize(count, BehaviorNode::Idle)
                }
                _ => return Err("Not a behavior composition.".into()),
            }
        }
        _ => return Err("Unknown behavior field.".into()),
    }
    Ok(())
}

fn remap(node: &mut BehaviorNode, from: usize, to: usize) {
    match node {
        BehaviorNode::Action(index) => {
            if *index == from {
                *index = to;
            } else if from < to && *index > from && *index <= to {
                *index -= 1;
            } else if to < from && *index >= to && *index < from {
                *index += 1;
            }
        }
        BehaviorNode::Sequence(children) | BehaviorNode::Priority(children) => {
            for child in children {
                remap(child, from, to);
            }
        }
        BehaviorNode::Idle => {}
    }
}
