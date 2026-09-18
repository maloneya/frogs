//! Small typed encounter rules. Authoring, admission and observations live here;
//! runtime instances never retain authored engine indices for body/source access.
use crate::door::Doors;
use crate::chest::Chests;
use crate::{DoorState, GameScene, SceneError};
use arpg_core::Report;
use arpg_sim::{
    Defeat, EntityId, Fnv, GameplayAccess, InteractionState, SceneId, SourceId, TraceSink,
};
use serde::Deserialize;
use std::num::NonZeroU32;

/// Bounded live encounter count, also bounding the presentation workload.
pub const MAX_ENCOUNTERS: usize = 16;

/// One authored activation condition.
#[derive(Clone, Copy, Debug, Deserialize)]
pub enum EncounterStart {
    /// Index of an explicit interactable body in this scene.
    Activated(usize),
}

/// Defeats from a scene-local group of sources, within a fixed tick budget.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefeatObjective {
    /// Distinct engine source indices; defines this encounter's enemy group.
    pub sources: Vec<usize>,
    /// Required combat defeats. Deserialization also rejects zero.
    pub count: NonZeroU32,
    /// Active simulation ticks, including the start tick and final allowed tick.
    pub within_ticks: NonZeroU32,
}

/// Supported objective rules; each variant owns its evaluation semantics.
#[derive(Clone, Debug, Deserialize)]
pub enum EncounterObjective {
    /// Count defeats from the declared source group.
    Defeat(DefeatObjective),
}

/// Typed effects. Targets are scene-local source, door or chest indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum EncounterAction {
    /// Enable an owned source at encounter start.
    EnableSource(usize),
    /// Unlock a gameplay door; opening still needs a fresh interaction.
    UnlockDoor(usize),
    /// Unlock a chest; a fresh interaction opens its lid.
    UnlockChest(usize),
}

/// A complete encounter composition, installed independently for each scene copy.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterSpec {
    /// Unique scene-local label, 1–48 printable ASCII characters.
    pub name: String,
    /// The activation rule.
    pub start: EncounterStart,
    /// Objective rule and tuning.
    pub objective: EncounterObjective,
    /// Effects applied before the first active simulation tick.
    pub on_start: Vec<EncounterAction>,
    /// Effects after a successful final simulation tick.
    #[serde(default)]
    pub on_success: Vec<EncounterAction>,
    /// Effects after a timeout. Missing endpoints do not run these effects.
    #[serde(default)]
    pub on_failure: Vec<EncounterAction>,
}

/// Terminal phases cannot restart except through a fresh scene instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum EncounterPhase {
    /// Waiting for activation.
    Ready,
    /// Counting defeats and consuming ticks.
    Running,
    /// Reached the target within the deadline.
    Succeeded,
    /// Spent the last allowed tick short of the target.
    Failed,
    /// A required endpoint disappeared before completion.
    Orphaned,
}

/// Shared observation vocabulary for HUD, reports and scenario assertions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterState {
    /// Current phase.
    pub phase: EncounterPhase,
    /// Qualifying defeats, capped at the target.
    pub defeated: u32,
    /// Objective target.
    pub target: u32,
    /// Unspent active ticks; freezes on a terminal transition.
    pub remaining_ticks: u32,
}

/// Diagnostic transitions; no gameplay logic reads these events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncounterEvent {
    /// Phase or progress changed.
    Progress {
        /// Owning scene.
        scene: SceneId,
        /// Authored encounter index.
        index: usize,
        /// State after the transition.
        state: EncounterState,
    },
    /// Chest changed state.
    Chest {
        /// Owning scene.
        scene: SceneId,
        /// Authored chest index.
        index: usize,
        /// New chest state.
        state: crate::ChestState,
    },
    /// Door changed state.
    Door {
        /// Owning scene.
        scene: SceneId,
        /// Authored door index.
        index: usize,
        /// New door state.
        state: DoorState,
    },
}
impl std::fmt::Display for EncounterEvent {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Progress {
                scene,
                index,
                state,
            } => write!(
                out,
                "encounter scene={scene} index={index} phase={:?} defeated={} remaining={}",
                state.phase, state.defeated, state.remaining_ticks
            ),
            Self::Chest { scene, index, state } => write!(out, "chest scene={scene} index={index} state={state:?}"),
            Self::Door {
                scene,
                index,
                state,
            } => write!(out, "door scene={scene} index={index} state={state:?}"),
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    EnableSource(SourceId),
    Door(usize),
    Chest(usize),
}
struct Encounter {
    owner: SceneId,
    name: String,
    start: EntityId,
    sources: Vec<SourceId>,
    doors: Vec<usize>,
    chests: Vec<usize>,
    on_start: Vec<Action>,
    on_success: Vec<Action>,
    on_failure: Vec<Action>,
    state: EncounterState,
}
#[derive(Default)]
pub(crate) struct Encounters {
    rows: Vec<Encounter>,
}

impl Encounters {
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub(crate) fn validate_add(&self, scene: &GameScene) -> Result<(), SceneError> {
        let invalid = |reason| Err(SceneError::Encounter(reason));
        if scene.encounters.len() > MAX_ENCOUNTERS - self.rows.len() {
            return invalid("too many live encounters");
        }
        let mut owned_sources = Vec::new();
        let mut owned_doors = Vec::new();
        let mut owned_chests = Vec::new();
        for (nth, spec) in scene.encounters.iter().enumerate() {
            if spec.name.is_empty()
                || spec.name.len() > 48
                || !spec.name.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
                || scene.encounters[..nth]
                    .iter()
                    .any(|other| other.name == spec.name)
            {
                return invalid("encounter names must be unique, printable ASCII, and 1–48 bytes");
            }
            let EncounterStart::Activated(body) = spec.start;
            if !scene
                .engine
                .bodies
                .get(body)
                .is_some_and(|body| body.what.interactable())
            {
                return invalid("encounter start must bind an explicit interactable body");
            }
            let EncounterObjective::Defeat(objective) = &spec.objective;
            if objective.sources.is_empty() {
                return invalid("defeat objective needs a source group");
            }
            for (index, &source) in objective.sources.iter().enumerate() {
                if !scene
                    .engine
                    .sources
                    .get(source)
                    .is_some_and(|source| !source.enabled && !source.what.fixed())
                {
                    return invalid("encounter sources must start disabled and produce enemies");
                }
                if objective.sources[..index].contains(&source)
                    || owned_sources.contains(&source)
                    || scene
                        .source_controls
                        .iter()
                        .any(|control| control.source == source)
                {
                    return invalid(
                        "a source may have only one encounter or activation controller",
                    );
                }
                if !spec
                    .on_start
                    .contains(&EncounterAction::EnableSource(source))
                {
                    return invalid("each objective source must be enabled on start");
                }
                owned_sources.push(source);
            }
            let mut local_doors = Vec::new();
            let mut local_chests = Vec::new();
            for (stage, actions) in [&spec.on_start, &spec.on_success, &spec.on_failure]
                .into_iter()
                .enumerate()
            {
                for (index, action) in actions.iter().enumerate() {
                    if actions[..index].contains(action) {
                        return invalid("duplicate or conflicting action target in one transition");
                    }
                    match *action {
                        EncounterAction::EnableSource(source) => {
                            if !objective.sources.contains(&source) {
                                return invalid(
                                    "source actions must target the encounter's source group",
                                );
                            }
                            if stage != 0 {
                                return invalid("terminal encounters cannot enable sources");
                            }
                        }
                        EncounterAction::UnlockChest(chest) => {
                            if chest >= scene.chests.len() || owned_chests.contains(&chest) {
                                return invalid("chest action must bind an exclusively owned scene chest");
                            }
                            if !local_chests.contains(&chest) { local_chests.push(chest); }
                        }
                        EncounterAction::UnlockDoor(door) => {
                            if door >= scene.doors.len() || owned_doors.contains(&door) {
                                return invalid(
                                    "door action must bind an exclusively owned scene door",
                                );
                            }
                            if !local_doors.contains(&door) {
                                local_doors.push(door);
                            }
                        }
                    }
                }
            }
            owned_doors.extend(local_doors);
            owned_chests.extend(local_chests);
        }
        Ok(())
    }
    pub(crate) fn install(
        &mut self,
        owner: SceneId,
        specs: &[EncounterSpec],
        bodies: &[EntityId],
        sources: &[SourceId],
    ) {
        for spec in specs {
            let EncounterStart::Activated(start) = spec.start;
            let EncounterObjective::Defeat(objective) = &spec.objective;
            let resolve = |actions: &[EncounterAction]| {
                actions
                    .iter()
                    .map(|action| match *action {
                        EncounterAction::EnableSource(index) => {
                            Action::EnableSource(sources[index])
                        }
                        EncounterAction::UnlockDoor(index) => Action::Door(index),
                        EncounterAction::UnlockChest(index) => Action::Chest(index),
                    })
                    .collect()
            };
            let mut doors = Vec::new();
            let mut chests = Vec::new();
            for action in spec
                .on_start
                .iter()
                .chain(&spec.on_success)
                .chain(&spec.on_failure)
            {
                if let EncounterAction::UnlockChest(index) = *action
                    && !chests.contains(&index) { chests.push(index); }
                if let EncounterAction::UnlockDoor(index) = *action
                    && !doors.contains(&index)
                {
                    doors.push(index);
                }
            }
            self.rows.push(Encounter {
                owner,
                name: spec.name.clone(),
                start: bodies[start],
                sources: objective
                    .sources
                    .iter()
                    .map(|&index| sources[index])
                    .collect(),
                doors,
                chests,
                on_start: resolve(&spec.on_start),
                on_success: resolve(&spec.on_success),
                on_failure: resolve(&spec.on_failure),
                state: EncounterState {
                    phase: EncounterPhase::Ready,
                    defeated: 0,
                    target: objective.count.get(),
                    remaining_ticks: objective.within_ticks.get(),
                },
            });
        }
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|row| row.owner != owner);
    }
    pub(crate) fn state(&self, owner: SceneId, nth: usize) -> Option<EncounterState> {
        self.rows
            .iter()
            .filter(|row| row.owner == owner)
            .nth(nth)
            .map(|row| row.state)
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, EncounterState)> {
        self.rows.iter().map(|row| (row.name.as_str(), row.state))
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for row in &self.rows {
            let Encounter {
                owner,
                name,
                start,
                sources,
                doors,
                chests,
                on_start,
                on_success,
                on_failure,
                state,
            } = row;
            owner.hash_into(out);
            start.hash_into(out);
            out.usize(name.len());
            for byte in name.bytes() {
                out.usize(byte as usize);
            }
            out.usize(sources.len());
            for source in sources {
                source.hash_into(out);
            }
            out.usize(doors.len());
            for door in doors {
                out.usize(*door);
            }
            out.usize(chests.len());
            for chest in chests { out.usize(*chest); }
            for actions in [on_start, on_success, on_failure] {
                out.usize(actions.len());
                for action in actions {
                    match action {
                        Action::EnableSource(id) => {
                            out.usize(0);
                            id.hash_into(out);
                        }
                        Action::Chest(index) => { out.usize(2); out.usize(*index); }
                        Action::Door(index) => {
                            out.usize(1);
                            out.usize(*index);
                        }
                    }
                }
            }
            out.usize(state.phase as usize);
            out.u64(u64::from(state.defeated));
            out.u64(u64::from(state.target));
            out.u64(u64::from(state.remaining_ticks));
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("encounters", |out| {
            for group in self.rows.chunk_by(|a, b| a.owner == b.owner) {
                for (index, row) in group.iter().enumerate() {
                    out.object(&format!("{}/{index}", row.owner), |out| {
                        out.text("name", &row.name);
                        out.text("start", &row.start.to_string());
                        out.text("phase", &format!("{:?}", row.state.phase));
                        out.int("defeated", u64::from(row.state.defeated));
                        out.int("target", u64::from(row.state.target));
                        out.int("remaining_ticks", u64::from(row.state.remaining_ticks));
                        out.object("sources", |out| {
                            for (index, source) in row.sources.iter().enumerate() {
                                out.text(&index.to_string(), &source.to_string());
                            }
                        });
                    });
                }
            }
        });
    }
}
fn effects(
    owner: SceneId,
    actions: &[Action],
    doors: &mut Doors,
    chests: &mut Chests,
    access: &mut GameplayAccess<'_>,
    trace: &mut TraceSink<'_, EncounterEvent>,
) {
    for action in actions {
        match *action {
            Action::EnableSource(id) => {
                let _ = access.set_source(id, true);
            }
            Action::Chest(index) => chests.unlock(owner, index, access, trace),
            Action::Door(index) => doors.unlock(owner, index, access, trace),
        }
    }
}
fn stop(row: &Encounter, access: &mut GameplayAccess<'_>) {
    for &source in &row.sources {
        let _ = access.set_source(source, false);
    }
}

/// Before simulation: consume prior interaction state. Validate all endpoints
/// before any effects, and stop surviving sources if a live encounter is orphaned.
pub(crate) fn begin(
    encounters: &mut Encounters,
    doors: &mut Doors,
    chests: &mut Chests,
    access: &mut GameplayAccess<'_>,
    trace: &mut TraceSink<'_, EncounterEvent>,
) {
    for group in encounters.rows.chunk_by_mut(|a, b| a.owner == b.owner) {
        for (index, row) in group.iter_mut().enumerate() {
            if !matches!(
                row.state.phase,
                EncounterPhase::Ready | EncounterPhase::Running
            ) {
                continue;
            }
            if row
                .sources
                .iter()
                .any(|&source| access.source(source).is_none())
                || row.chests.iter().any(|&chest| !chests.available(row.owner, chest, access))
                || row
                    .doors
                    .iter()
                    .any(|&door| !doors.available(row.owner, door, access))
                || (row.state.phase == EncounterPhase::Ready
                    && access.interaction(row.start).is_none())
            {
                row.state.phase = EncounterPhase::Orphaned;
                stop(row, access);
            } else if row.state.phase == EncounterPhase::Ready
                && access.interaction(row.start) == Some(InteractionState::Activated)
            {
                row.state.phase = EncounterPhase::Running;
                effects(row.owner, &row.on_start, doors, chests, access, trace);
            } else {
                continue;
            }
            trace.emit(EncounterEvent::Progress {
                scene: row.owner,
                index,
                state: row.state,
            });
        }
    }
}

/// After simulation: consume this tick's lossless defeat outcomes exactly once.
/// Spend one tick, then prefer success over timeout on the final allowed tick.
pub(crate) fn finish(
    encounters: &mut Encounters,
    doors: &mut Doors,
    chests: &mut Chests,
    defeats: &[Defeat],
    access: &mut GameplayAccess<'_>,
    trace: &mut TraceSink<'_, EncounterEvent>,
) {
    for group in encounters.rows.chunk_by_mut(|a, b| a.owner == b.owner) {
        for (index, row) in group.iter_mut().enumerate() {
            if row.state.phase != EncounterPhase::Running {
                continue;
            }
            let previous = row.state.defeated;
            let count = defeats
                .iter()
                .filter(|defeat| {
                    defeat
                        .source
                        .is_some_and(|source| row.sources.contains(&source))
                })
                .count();
            row.state.defeated = row
                .state
                .defeated
                .saturating_add(count as u32)
                .min(row.state.target);
            row.state.remaining_ticks -= 1;
            if row.state.defeated == row.state.target {
                row.state.phase = EncounterPhase::Succeeded;
            } else if row.state.remaining_ticks == 0 {
                row.state.phase = EncounterPhase::Failed;
            }
            if row.state.phase != EncounterPhase::Running {
                stop(row, access);
                let actions = if row.state.phase == EncounterPhase::Succeeded {
                    &row.on_success
                } else {
                    &row.on_failure
                };
                effects(row.owner, actions, doors, chests, access, trace);
            }
            if row.state.defeated != previous || row.state.phase != EncounterPhase::Running {
                trace.emit(EncounterEvent::Progress {
                    scene: row.owner,
                    index,
                    state: row.state,
                });
            }
        }
    }
}
