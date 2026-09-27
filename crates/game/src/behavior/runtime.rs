//! Deterministic action ownership. Physics consumes commands and owns all movement.

use super::coordination::Coordination;
use super::definition::{
    ActionDefinition, ActorDefinition, ActorRole, BehaviorNode, Destination, MAX_ACTIONS,
    MAX_BEHAVIOR_NODES, TargetSelection,
};
use arpg_core::Report;
pub use arpg_sim::ActionOutcome;
use arpg_sim::{
    ActionControls, ActionExecution, ActionSchedule, ActorView, AttackPhase, Dt, EntityId, Fnv,
    InteractionState, Interruption, MotorCommand, SceneId,
};
use glam::Vec2;
use serde::Deserialize;
use std::{num::NonZeroU8, sync::Arc};

const VACANT: usize = usize::MAX;
const HOLD_ACCELERATION: f32 = 30.0;

/// The operation currently controlling an actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ActionKind {
    /// No active operation.
    Idle,
    /// Interruptible approach movement.
    Move,
    /// A timed hold.
    Wait,
    /// An interaction request.
    Interact,
    /// A direction-committed close strike.
    Swipe,
    /// Travel toward a committed landing point.
    Leap,
}
impl ActionKind {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Move => "move",
            Self::Wait => "wait",
            Self::Interact => "interact",
            Self::Swipe => "swipe",
            Self::Leap => "leap",
        }
    }
    fn combat(self) -> bool {
        matches!(self, Self::Swipe | Self::Leap)
    }
}

/// Read-only action facts; observations cannot alter decisions or their timing.
#[derive(Clone, Copy, Debug)]
pub struct ActionStatus {
    /// Stable actor identity.
    pub body: EntityId,
    /// Active operation.
    pub kind: ActionKind,
    /// Commitment phase; Move, Wait and Interact use Active.
    pub phase: AttackPhase,
    /// Completed ticks within this phase.
    pub elapsed: u32,
    /// Duration of this phase; zero for open-ended movement or Idle.
    pub duration: u32,
    /// Committed facing direction for combat, desired direction for movement.
    pub direction: Vec2,
    /// Ground movement destination or committed damage-disc centre.
    pub centre: Vec2,
    /// Damage-disc radius, excluding target radius; zero for noncombat actions.
    pub radius: f32,
    /// Index in the shared definition, absent while Idle.
    pub action: Option<usize>,
    /// Explicit committed target identity, if any.
    pub target: Option<EntityId>,
    /// Current or most recently retired operation outcome.
    pub outcome: ActionOutcome,
    /// Outcome of the last retired action, retained while another action runs.
    pub previous_outcome: ActionOutcome,
}

/// Narrow effects submitted after physics. The shared owner revalidates each one.
#[derive(Clone, Copy)]
pub(crate) enum ActionEffect {
    Damage {
        actor: EntityId,
        target: EntityId,
        amount: NonZeroU8,
    },
    Interact {
        actor: EntityId,
        target: EntityId,
    },
}
impl ActionEffect {
    pub(crate) fn actor(self) -> EntityId {
        match self {
            Self::Damage { actor, .. } | Self::Interact { actor, .. } => actor,
        }
    }
}

#[derive(Clone, Copy)]
struct Running {
    node: usize,
    action: usize,
    execution: ActionExecution,
    target: Option<EntityId>,
    direction: Vec2,
    destination: Vec2,
    hit: bool,
}

struct Actor {
    body: EntityId,
    owner: Option<SceneId>,
    definition: Arc<ActorDefinition>,
    // Derived once from immutable shared content; runtime progress is hashed separately.
    definition_hash: u64,
    running: Option<Running>,
    cursors: [u8; MAX_BEHAVIOR_NODES],
    cooldowns: [u32; MAX_ACTIONS],
    outcome: ActionOutcome,
}
impl Actor {
    fn new(body: EntityId, owner: Option<SceneId>, definition: Arc<ActorDefinition>) -> Self {
        let mut hash = Fnv::default();
        definition.hash(&mut hash);
        Self {
            body,
            owner,
            definition,
            definition_hash: hash.finish(),
            running: None,
            cursors: [0; MAX_BEHAVIOR_NODES],
            cooldowns: [0; MAX_ACTIONS],
            outcome: ActionOutcome::None,
        }
    }
    fn kind(&self) -> ActionKind {
        self.running.as_ref().map_or(ActionKind::Idle, |running| {
            kind(&self.definition.actions[running.action])
        })
    }
    fn complete(&mut self, succeeded: bool) {
        if let Some(mut running) = self.running.take() {
            running.execution.complete(succeeded);
            finish_node(
                &self.definition.behavior,
                0,
                running.node,
                succeeded,
                &mut self.cursors,
            );
        }
        self.outcome = if succeeded {
            ActionOutcome::Succeeded
        } else {
            ActionOutcome::Failed
        };
    }
    fn cancel(&mut self) {
        if let Some(running) = &mut self.running {
            running.execution.cancel();
        }
        self.running = None;
        self.cursors.fill(0);
        self.outcome = ActionOutcome::Cancelled;
    }
    fn status(&self, view: ActorView<'_>) -> Option<ActionStatus> {
        let origin = view.position(self.body)?;
        let Some(running) = &self.running else {
            return Some(ActionStatus {
                body: self.body,
                kind: ActionKind::Idle,
                phase: AttackPhase::Idle,
                elapsed: 0,
                duration: 0,
                direction: Vec2::Y,
                centre: origin,
                radius: 0.0,
                action: None,
                target: None,
                outcome: self.outcome,
                previous_outcome: self.outcome,
            });
        };
        let action = &self.definition.actions[running.action];
        let (phase, elapsed, duration) = running.execution.phase();
        let (centre, radius) = match action {
            ActionDefinition::Swipe(settings) => (
                origin + running.direction * settings.offset,
                settings.radius,
            ),
            ActionDefinition::Leap(settings) => (running.destination, settings.radius),
            ActionDefinition::Move(_)
            | ActionDefinition::Wait { .. }
            | ActionDefinition::Interact { .. } => (running.destination, 0.0),
        };
        Some(ActionStatus {
            body: self.body,
            kind: kind(action),
            phase,
            elapsed,
            duration,
            direction: running.direction,
            centre,
            radius,
            action: Some(running.action),
            target: running.target,
            outcome: running.execution.outcome(),
            previous_outcome: self.outcome,
        })
    }
}

pub(crate) struct Actors {
    rows: Vec<Actor>,
    sparse: Vec<usize>,
    coordination: Coordination,
    drives: Vec<MotorCommand>,
    effects: Vec<ActionEffect>,
    targets: Vec<EntityId>,
}
impl Default for Actors {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            sparse: Vec::new(),
            coordination: Coordination::default(),
            drives: Vec::with_capacity(arpg_sim::MAX_BODIES),
            effects: Vec::with_capacity(arpg_sim::MAX_BODIES),
            targets: Vec::with_capacity(arpg_sim::MAX_BODIES),
        }
    }
}
impl Actors {
    pub(crate) fn install(
        &mut self,
        id: EntityId,
        owner: Option<SceneId>,
        definition: Arc<ActorDefinition>,
    ) {
        assert!(
            self.index(id).is_none(),
            "an actor is installed once at physical admission"
        );
        let slot = id.diagnostic_identity().0 as usize;
        if self.sparse.len() <= slot {
            self.sparse.resize(slot + 1, VACANT);
        }
        self.sparse[slot] = self.rows.len();
        self.rows.push(Actor::new(id, owner, definition));
    }
    fn index(&self, id: EntityId) -> Option<usize> {
        let row = *self.sparse.get(id.diagnostic_identity().0 as usize)?;
        self.rows
            .get(row)
            .filter(|actor| actor.body == id)
            .map(|_| row)
    }
    pub(crate) fn definition(&self, id: EntityId) -> Option<&ActorDefinition> {
        self.index(id).map(|row| self.rows[row].definition.as_ref())
    }
    pub(crate) fn replace(&mut self, id: EntityId, definition: Arc<ActorDefinition>) {
        let row = self.index(id).expect("replacement actor admitted by Game");
        let owner = self.rows[row].owner;
        let was_running = self.rows[row].running.is_some();
        self.rows[row] = Actor::new(id, owner, definition);
        if was_running {
            self.rows[row].outcome = ActionOutcome::Cancelled;
        }
    }
    pub(crate) fn remove(&mut self, id: EntityId) {
        let Some(row) = self.index(id) else { return };
        self.rows.remove(row);
        self.reindex();
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|actor| actor.owner != Some(owner));
        self.reindex();
    }
    pub(crate) fn retire(&mut self, view: ActorView<'_>) {
        let previous = self.rows.len();
        self.rows
            .retain(|actor| view.alive(actor.body) && view.health(actor.body) != Some(0));
        if previous != self.rows.len() {
            self.reindex();
        }
    }
    fn reindex(&mut self) {
        self.sparse.fill(VACANT);
        for (row, actor) in self.rows.iter().enumerate() {
            self.sparse[actor.body.diagnostic_identity().0 as usize] = row;
        }
        self.coordination.retain_count(self.rows.len());
        if self.rows.is_empty() {
            self.drives.clear();
            self.effects.clear();
            self.targets.clear();
        }
    }
    pub(crate) fn hostile_ids(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.rows
            .iter()
            .filter(|actor| actor.definition.role == ActorRole::Hostile)
            .map(|actor| actor.body)
    }
    pub(crate) fn population(&self) -> usize {
        self.hostile_ids().count()
    }
    pub(crate) fn drives(&self) -> &[MotorCommand] {
        &self.drives
    }
    pub(crate) fn effects(&self) -> &[ActionEffect] {
        &self.effects
    }
    pub(crate) fn targets(&self) -> &[EntityId] {
        &self.targets
    }
    pub(crate) fn status(&self, id: EntityId, view: ActorView<'_>) -> Option<ActionStatus> {
        self.index(id).and_then(|row| self.rows[row].status(view))
    }
    pub(crate) fn statuses<'a>(
        &'a self,
        view: ActorView<'a>,
    ) -> impl Iterator<Item = ActionStatus> + 'a {
        self.rows.iter().filter_map(move |actor| actor.status(view))
    }

    /// Admission already ran. Decisions observe the same post-walk positions for
    /// every actor; only the motor/physics passes that follow may move them.
    pub(crate) fn prepare(&mut self, view: ActorView<'_>, dt: Dt) {
        self.drives.clear();
        self.effects.clear();
        self.targets.clear();
        self.targets.extend(
            self.rows
                .iter()
                .filter(|actor| actor.definition.role == ActorRole::Hostile)
                .map(|actor| actor.body),
        );
        self.coordination.begin_tick();
        let mut committed = 0;
        for index in 0..self.rows.len() {
            let actor = &mut self.rows[index];
            for cooldown in &mut actor.cooldowns[..actor.definition.actions.len()] {
                *cooldown = cooldown.saturating_sub(1);
            }
            let actor = &self.rows[index];
            let Some(running) = &actor.running else {
                continue;
            };
            if !running.execution.is_running() {
                let succeeded = running.execution.outcome() == ActionOutcome::Succeeded;
                self.rows[index].complete(succeeded);
                continue;
            }
            let selection = match &actor.definition.actions[running.action] {
                ActionDefinition::Move(settings) => match settings.destination {
                    Destination::Target(selection) => Some(selection),
                    Destination::Point(_) => None,
                },
                ActionDefinition::Swipe(settings) => Some(settings.target),
                ActionDefinition::Leap(settings) => Some(settings.target),
                ActionDefinition::Interact { target } => Some(*target),
                ActionDefinition::Wait { .. } => None,
            };
            if running.target.is_some_and(|target| {
                selection.map_or_else(
                    || view.position(target).is_none(),
                    |selection| !eligible(selection, actor.body, target, view, self),
                )
            }) {
                self.rows[index].cancel();
                continue;
            }
            let actor = &mut self.rows[index];
            let running = actor
                .running
                .as_mut()
                .expect("running action checked above");
            running.execution.advance();
            let outcome = running.execution.outcome();
            let reached = match &actor.definition.actions[running.action] {
                ActionDefinition::Move(settings) => {
                    let destination = match settings.destination {
                        Destination::Point(point) => Some(Vec2::from(point)),
                        Destination::Target(_) => {
                            running.target.and_then(|target| view.position(target))
                        }
                    };
                    view.position(actor.body).zip(destination).is_some_and(
                        |(origin, destination)| {
                            running.destination = destination;
                            running.direction = (destination - origin).normalize_or(Vec2::Y);
                            arrived(origin, destination, settings.stop_distance)
                        },
                    )
                }
                ActionDefinition::Wait { .. }
                | ActionDefinition::Interact { .. }
                | ActionDefinition::Swipe(_)
                | ActionDefinition::Leap(_) => false,
            };
            if outcome != ActionOutcome::Running || reached {
                actor.complete(reached || outcome == ActionOutcome::Succeeded);
            }
            committed += usize::from(actor.kind().combat());
        }
        let count = self.rows.len();
        let start = self.coordination.start_index();
        for offset in 0..count {
            let index = (start + offset) % count;
            let actor = &mut self.rows[index];
            if actor
                .running
                .as_ref()
                .is_none_or(|running| running.execution.interruptible())
            {
                let definition = Arc::clone(&actor.definition);
                let prior = actor.running.as_ref().map(|running| running.node);
                let can_commit = self.coordination.can_start(committed);
                let selection = select(
                    &definition.behavior,
                    0,
                    actor,
                    view,
                    &self.targets,
                    can_commit,
                );
                let candidate = match selection {
                    Selection::Run(running) => Some(running),
                    Selection::Success => {
                        actor.outcome = ActionOutcome::Succeeded;
                        None
                    }
                    Selection::Failure => {
                        if prior.is_some() {
                            actor.outcome = ActionOutcome::Failed;
                        }
                        None
                    }
                };
                if let (Some(old), Some(new)) = (prior, candidate.as_ref())
                    && old != new.node
                {
                    reset_abandoned(&definition.behavior, 0, old, new.node, &mut actor.cursors);
                    actor.outcome = ActionOutcome::Cancelled;
                }
                actor.running = candidate;
                if let Some(running) = &actor.running
                    && kind(&definition.actions[running.action]).combat()
                {
                    committed += 1;
                    self.coordination.granted(index, count);
                    if let ActionDefinition::Leap(settings) = &definition.actions[running.action] {
                        actor.cooldowns[running.action] = settings.cooldown;
                    }
                }
            }
            if let Some(command) = motor(actor, view, dt) {
                self.drives.push(command);
            }
        }
    }

    /// Effects observe final physical positions; the sim damage authority still
    /// refuses stale, defeated and ineligible targets at effect application.
    pub(crate) fn resolve(&mut self, view: ActorView<'_>) {
        self.effects.clear();
        for actor in &mut self.rows {
            let Some(running) = &mut actor.running else {
                continue;
            };
            let action = &actor.definition.actions[running.action];
            if matches!(
                action,
                ActionDefinition::Move(_) | ActionDefinition::Wait { .. }
            ) || !view.alive(actor.body)
                || view.health(actor.body) == Some(0)
            {
                continue;
            }
            let Some(target) = running.target else {
                continue;
            };
            let (Some(origin), Some(destination)) =
                (view.position(actor.body), view.position(target))
            else {
                continue;
            };
            match action {
                ActionDefinition::Interact { .. } => {
                    if running.execution.is_running() {
                        self.effects.push(ActionEffect::Interact {
                            actor: actor.body,
                            target,
                        });
                    }
                }
                ActionDefinition::Swipe(settings) => {
                    let (phase, _, _) = running.execution.phase();
                    let centre = origin + running.direction * settings.offset;
                    let reach = settings.radius + target_radius(view, target);
                    if !running.hit
                        && phase == AttackPhase::Active
                        && view.health(target).is_some_and(|health| health > 0)
                        && centre.distance_squared(destination) <= reach * reach
                        && view.clear_path(origin, destination)
                    {
                        self.effects.push(ActionEffect::Damage {
                            actor: actor.body,
                            target,
                            amount: settings.damage,
                        });
                    }
                }
                ActionDefinition::Leap(settings) => {
                    let (phase, elapsed, _) = running.execution.phase();
                    let reach = settings.radius + target_radius(view, target);
                    if !running.hit
                        && phase == AttackPhase::Recovery
                        && elapsed == 0
                        && view.health(target).is_some_and(|health| health > 0)
                        && running.destination.distance_squared(destination) <= reach * reach
                        && origin.distance_squared(destination) <= reach * reach
                        && view.clear_path(origin, destination)
                    {
                        self.effects.push(ActionEffect::Damage {
                            actor: actor.body,
                            target,
                            amount: settings.damage,
                        });
                    }
                }
                ActionDefinition::Move(_) | ActionDefinition::Wait { .. } => {}
            }
        }
    }
    pub(crate) fn effect_result(&mut self, id: EntityId, accepted: bool) {
        let Some(row) = self.index(id) else { return };
        let actor = &mut self.rows[row];
        let Some(running) = &mut actor.running else {
            return;
        };
        match &actor.definition.actions[running.action] {
            ActionDefinition::Interact { .. } => running.execution.complete(accepted),
            ActionDefinition::Swipe(_) | ActionDefinition::Leap(_) => running.hit |= accepted,
            ActionDefinition::Move(_) | ActionDefinition::Wait { .. } => {}
        }
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        self.coordination.hash(out);
        for actor in &self.rows {
            actor.body.hash_into(out);
            out.usize(usize::from(actor.owner.is_some()));
            if let Some(owner) = actor.owner {
                owner.hash_into(out);
            }
            out.u64(actor.definition_hash);
            out.usize(actor.outcome as usize);
            for cursor in actor.cursors {
                out.usize(usize::from(cursor));
            }
            for cooldown in actor.cooldowns {
                out.usize(cooldown as usize);
            }
            out.usize(usize::from(actor.running.is_some()));
            if let Some(running) = &actor.running {
                out.usize(running.node);
                out.usize(running.action);
                running.execution.hash_into(out);
                out.usize(usize::from(running.target.is_some()));
                if let Some(target) = running.target {
                    target.hash_into(out);
                }
                out.f32(running.direction.x);
                out.f32(running.direction.y);
                out.f32(running.destination.x);
                out.f32(running.destination.y);
                out.usize(usize::from(running.hit));
            }
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("actor_definitions", |out| {
            let mut seen = std::collections::BTreeSet::new();
            for actor in &self.rows {
                if seen.insert(actor.definition_hash) {
                    out.object(&format!("{:016x}", actor.definition_hash), |out| {
                        actor.definition.report(out)
                    });
                }
            }
        });
        out.object("actors", |out| {
            out.int("count", self.rows.len() as u64);
            out.int("hostile_population", self.population() as u64);
            self.coordination.report(out);
            for actor in &self.rows {
                out.object(&actor.body.to_string(), |out| {
                    out.text(
                        "role",
                        match actor.definition.role {
                            ActorRole::Hostile => "hostile",
                            ActorRole::Civilian => "civilian",
                            ActorRole::Prop => "prop",
                        },
                    );
                    out.text("action", actor.kind().label());
                    out.text(
                        "definition_hash",
                        &format!("{:016x}", actor.definition_hash),
                    );
                    out.int("action_count", actor.definition.actions.len() as u64);
                    out.bool("character", actor.definition.physical.is_character());
                    out.bool("fixed", actor.definition.physical.fixed());
                    out.bool("interactable", actor.definition.physical.interactable());
                    out.int(
                        "initial_health",
                        actor
                            .definition
                            .physical
                            .hit_points()
                            .map_or(0, |health| u64::from(health.get())),
                    );
                    out.text(
                        "previous_outcome",
                        match actor.outcome {
                            ActionOutcome::None => "none",
                            ActionOutcome::Running => "running",
                            ActionOutcome::Succeeded => "succeeded",
                            ActionOutcome::Failed => "failed",
                            ActionOutcome::Cancelled => "cancelled",
                        },
                    );
                    if let Some(running) = &actor.running {
                        let (phase, elapsed, duration) = running.execution.phase();
                        out.int("action_index", running.action as u64);
                        out.int("node", running.node as u64);
                        out.object("execution", |out| running.execution.report(out));
                        out.text(
                            "phase",
                            match phase {
                                AttackPhase::Idle => "idle",
                                AttackPhase::Startup => "startup",
                                AttackPhase::Active => "active",
                                AttackPhase::Recovery => "recovery",
                            },
                        );
                        out.int("elapsed_ticks", u64::from(elapsed));
                        out.int("duration_ticks", u64::from(duration));
                        if let Some(target) = running.target {
                            out.text("target", &target.to_string());
                        }
                        out.num("direction_x", running.direction.x);
                        out.num("direction_z", running.direction.y);
                        out.num("destination_x", running.destination.x);
                        out.num("destination_z", running.destination.y);
                        out.bool("hit", running.hit);
                        if !running.execution.is_running() {
                            out.bool(
                                "effect_accepted",
                                running.execution.outcome() == ActionOutcome::Succeeded,
                            );
                        }
                    }
                    out.object("cooldowns", |out| {
                        for (index, cooldown) in actor
                            .cooldowns
                            .iter()
                            .take(actor.definition.actions.len())
                            .enumerate()
                        {
                            out.int(&index.to_string(), u64::from(*cooldown));
                        }
                    });
                    out.object("sequence_progress", |out| {
                        for (index, cursor) in actor
                            .cursors
                            .iter()
                            .take(span(&actor.definition.behavior))
                            .enumerate()
                        {
                            out.int(&index.to_string(), u64::from(*cursor));
                        }
                    });
                });
            }
        });
    }
}

fn kind(action: &ActionDefinition) -> ActionKind {
    match action {
        ActionDefinition::Move(_) => ActionKind::Move,
        ActionDefinition::Wait { .. } => ActionKind::Wait,
        ActionDefinition::Interact { .. } => ActionKind::Interact,
        ActionDefinition::Swipe(_) => ActionKind::Swipe,
        ActionDefinition::Leap(_) => ActionKind::Leap,
    }
}
fn execution(action: &ActionDefinition) -> ActionExecution {
    let (schedule, controls, interruption) = match action {
        ActionDefinition::Move(_) => (
            ActionSchedule::Open,
            ActionControls::Movement,
            Interruption::Interruptible,
        ),
        ActionDefinition::Wait { ticks } => (
            ActionSchedule::timed(0, *ticks, 0).expect("validated wait"),
            ActionControls::Movement,
            Interruption::Committed,
        ),
        ActionDefinition::Interact { .. } => (
            ActionSchedule::Open,
            ActionControls::Movement,
            Interruption::Committed,
        ),
        ActionDefinition::Swipe(settings) => (
            ActionSchedule::timed(
                settings.timing.startup,
                settings.timing.active,
                settings.timing.recovery,
            )
            .expect("validated swipe"),
            ActionControls::MovementAndFacing,
            Interruption::Committed,
        ),
        ActionDefinition::Leap(settings) => (
            ActionSchedule::timed(
                settings.timing.startup,
                settings.timing.active,
                settings.timing.recovery,
            )
            .expect("validated leap"),
            ActionControls::MovementAndFacing,
            Interruption::Committed,
        ),
    };
    ActionExecution::new(schedule, controls, interruption)
}
fn target_radius(view: ActorView<'_>, target: EntityId) -> f32 {
    view.radius(target)
        .expect("effect target position was resolved")
}
fn target(
    selection: TargetSelection,
    actor: EntityId,
    view: ActorView<'_>,
    hostiles: &[EntityId],
) -> Option<EntityId> {
    let origin = view.position(actor)?;
    match selection {
        TargetSelection::Player => view.position(view.player_id()).map(|_| view.player_id()),
        TargetSelection::NearestHostile => nearest(
            origin,
            hostiles
                .iter()
                .copied()
                .filter(|id| *id != actor)
                .filter(|id| view.health(*id).is_some_and(|health| health > 0))
                .filter_map(|id| view.position(id).map(|position| (id, position))),
        ),
        TargetSelection::NearestInteraction => nearest(
            origin,
            view.interactions()
                .filter(|(id, _, state)| *id != actor && *state == InteractionState::Ready)
                .map(|(id, position, _)| (id, position)),
        ),
    }
}
fn eligible(
    selection: TargetSelection,
    actor: EntityId,
    target: EntityId,
    view: ActorView<'_>,
    actors: &Actors,
) -> bool {
    if actor == target || view.position(target).is_none() {
        return false;
    }
    match selection {
        TargetSelection::Player => target == view.player_id(),
        TargetSelection::NearestHostile => {
            actors
                .definition(target)
                .is_some_and(|definition| definition.role == ActorRole::Hostile)
                && view.health(target).is_some_and(|health| health > 0)
        }
        TargetSelection::NearestInteraction => {
            view.interaction(target) == Some(InteractionState::Ready)
        }
    }
}
fn nearest(origin: Vec2, candidates: impl Iterator<Item = (EntityId, Vec2)>) -> Option<EntityId> {
    candidates
        .map(|(id, position)| (id, origin.distance_squared(position)))
        .min_by(|(left, a), (right, b)| {
            a.total_cmp(b)
                .then_with(|| left.diagnostic_identity().cmp(&right.diagnostic_identity()))
        })
        .map(|(id, _)| id)
}

enum Selection {
    Run(Running),
    Success,
    Failure,
}

fn select(
    node: &BehaviorNode,
    index: usize,
    actor: &mut Actor,
    view: ActorView<'_>,
    hostiles: &[EntityId],
    can_commit: bool,
) -> Selection {
    match node {
        BehaviorNode::Idle => Selection::Failure,
        BehaviorNode::Action(action) => admit(index, *action, actor, view, hostiles, can_commit),
        BehaviorNode::Sequence(children) => {
            let mut child_index = index + 1;
            for (nth, child) in children.iter().enumerate() {
                if nth >= usize::from(actor.cursors[index]) {
                    match select(child, child_index, actor, view, hostiles, can_commit) {
                        Selection::Run(running) => return Selection::Run(running),
                        Selection::Failure => {
                            actor.cursors[index..index + span(node)].fill(0);
                            return Selection::Failure;
                        }
                        Selection::Success => actor.cursors[index] = (nth + 1) as u8,
                    }
                }
                child_index += span(child);
            }
            actor.cursors[index..index + span(node)].fill(0);
            Selection::Success
        }
        BehaviorNode::Priority(children) => {
            let mut child_index = index + 1;
            for child in children {
                match select(child, child_index, actor, view, hostiles, can_commit) {
                    Selection::Run(running) => return Selection::Run(running),
                    Selection::Success => return Selection::Success,
                    Selection::Failure => {}
                }
                child_index += span(child);
            }
            Selection::Failure
        }
    }
}
fn admit(
    node: usize,
    action_index: usize,
    actor: &Actor,
    view: ActorView<'_>,
    hostiles: &[EntityId],
    can_commit: bool,
) -> Selection {
    let action = &actor.definition.actions[action_index];
    if actor.cooldowns[action_index] != 0 {
        return Selection::Failure;
    }
    // Retained movement was refreshed against this decision snapshot above.
    // Reconsider higher-priority nodes, but do not resolve the same move twice.
    if matches!(action, ActionDefinition::Move(_))
        && let Some(running) = actor
            .running
            .as_ref()
            .filter(|running| running.node == node)
    {
        return Selection::Run(*running);
    }
    let Some(origin) = view.position(actor.body) else {
        return Selection::Failure;
    };
    let mut running = Running {
        node,
        action: action_index,
        execution: execution(action),
        target: None,
        direction: Vec2::Y,
        destination: origin,
        hit: false,
    };
    match action {
        ActionDefinition::Move(settings) => {
            let destination = match settings.destination {
                Destination::Point(point) => Vec2::from(point),
                Destination::Target(selection) => {
                    let Some(id) = running
                        .target
                        .or_else(|| target(selection, actor.body, view, hostiles))
                    else {
                        return Selection::Failure;
                    };
                    let Some(position) = view.position(id) else {
                        return Selection::Failure;
                    };
                    running.target = Some(id);
                    position
                }
            };
            if arrived(origin, destination, settings.stop_distance) {
                return Selection::Success;
            }
            running.destination = destination;
            running.direction = (destination - origin).normalize_or(Vec2::Y);
        }
        ActionDefinition::Wait { .. } => {}
        ActionDefinition::Interact { target: selection } => {
            let Some(id) = target(*selection, actor.body, view, hostiles) else {
                return Selection::Failure;
            };
            let Some(position) = view.position(id) else {
                return Selection::Failure;
            };
            if !view.can_interact(actor.body, id) {
                return Selection::Failure;
            }
            running.target = Some(id);
            running.destination = position;
        }
        ActionDefinition::Swipe(settings) => {
            if !can_commit {
                return Selection::Failure;
            }
            let Some(id) = target(settings.target, actor.body, view, hostiles) else {
                return Selection::Failure;
            };
            let Some(position) = view.position(id) else {
                return Selection::Failure;
            };
            if view.health(id).is_none_or(|health| health == 0)
                || origin.distance_squared(position) > settings.reach * settings.reach
                || !view.clear_path(origin, position)
            {
                return Selection::Failure;
            }
            running.target = Some(id);
            running.direction = (position - origin).normalize_or(Vec2::Y);
            running.destination = position;
        }
        ActionDefinition::Leap(settings) => {
            if !can_commit {
                return Selection::Failure;
            }
            let Some(id) = target(settings.target, actor.body, view, hostiles) else {
                return Selection::Failure;
            };
            let Some(position) = view.position(id) else {
                return Selection::Failure;
            };
            let distance = origin.distance(position);
            if view.health(id).is_none_or(|health| health == 0)
                || !(settings.min_range..=settings.max_range).contains(&distance)
                || !view.clear_path(origin, position)
            {
                return Selection::Failure;
            }
            running.target = Some(id);
            running.direction = (position - origin).normalize_or(Vec2::Y);
            running.destination = position;
        }
    }
    Selection::Run(running)
}
fn motor(actor: &Actor, view: ActorView<'_>, dt: Dt) -> Option<MotorCommand> {
    let running = actor.running.as_ref()?;
    if !running.execution.holds(ActionControls::Movement) {
        return None;
    }
    let origin = view.position(actor.body)?;
    if !view.movable(actor.body) {
        return None;
    }
    let action = &actor.definition.actions[running.action];
    let (velocity, acceleration) = match action {
        ActionDefinition::Move(settings) => {
            let difference = running.destination - origin;
            let length = difference.length();
            let distance = (length - settings.stop_distance).max(0.0);
            let speed = settings.speed.min(arpg_sim::stopping_speed(
                distance,
                dt,
                settings.acceleration,
            ));
            (
                if length > 0.0 {
                    difference / length * speed
                } else {
                    Vec2::ZERO
                },
                settings.acceleration,
            )
        }
        ActionDefinition::Leap(settings) => {
            let (phase, elapsed, duration) = running.execution.phase();
            if phase == AttackPhase::Active {
                let remaining = (duration - elapsed) as f32 * Dt::SECS;
                (
                    ((running.destination - origin) / remaining).clamp_length_max(settings.speed),
                    settings.acceleration,
                )
            } else {
                (Vec2::ZERO, HOLD_ACCELERATION)
            }
        }
        ActionDefinition::Wait { .. }
        | ActionDefinition::Interact { .. }
        | ActionDefinition::Swipe(_) => (Vec2::ZERO, HOLD_ACCELERATION),
    };
    Some(MotorCommand::new(actor.body, velocity, acceleration).expect("validated action motor"))
}

fn arrived(origin: Vec2, destination: Vec2, distance: f32) -> bool {
    // Braking can reach the nearest representable world point just outside the
    // requested radius. Bound arrival rounding by the stored coordinates' scale.
    let scale = origin.abs().max(destination.abs()).max_element().max(1.0);
    let reach = distance + 4.0 * f32::EPSILON * scale;
    origin.distance_squared(destination) <= reach * reach
}

fn span(node: &BehaviorNode) -> usize {
    match node {
        BehaviorNode::Idle | BehaviorNode::Action(_) => 1,
        BehaviorNode::Sequence(children) | BehaviorNode::Priority(children) => {
            1 + children.iter().map(span).sum::<usize>()
        }
    }
}
/// Propagate a completed leaf through its active composition. Sequences retain
/// their next child; a completed sequence or priority becomes a parent outcome.
fn finish_node(
    node: &BehaviorNode,
    index: usize,
    finished: usize,
    succeeded: bool,
    cursors: &mut [u8; MAX_BEHAVIOR_NODES],
) -> Option<bool> {
    if index == finished {
        return Some(succeeded);
    }
    match node {
        BehaviorNode::Idle | BehaviorNode::Action(_) => None,
        BehaviorNode::Sequence(children) | BehaviorNode::Priority(children) => {
            let mut child_index = index + 1;
            for (nth, child) in children.iter().enumerate() {
                let width = span(child);
                if (child_index..child_index + width).contains(&finished) {
                    let outcome = finish_node(child, child_index, finished, succeeded, cursors)?;
                    if matches!(node, BehaviorNode::Priority(_)) {
                        return Some(outcome);
                    }
                    if !outcome {
                        cursors[index..index + span(node)].fill(0);
                        return Some(false);
                    }
                    if nth + 1 < children.len() {
                        cursors[index] = (nth + 1) as u8;
                        return None;
                    }
                    cursors[index..index + span(node)].fill(0);
                    return Some(true);
                }
                child_index += width;
            }
            None
        }
    }
}
fn reset_abandoned(
    node: &BehaviorNode,
    index: usize,
    old: usize,
    new: usize,
    cursors: &mut [u8; MAX_BEHAVIOR_NODES],
) {
    let width = span(node);
    if !(index..index + width).contains(&old) {
        return;
    }
    if !(index..index + width).contains(&new) {
        cursors[index..index + width].fill(0);
        return;
    }
    match node {
        BehaviorNode::Idle | BehaviorNode::Action(_) => {}
        BehaviorNode::Sequence(children) | BehaviorNode::Priority(children) => {
            let mut child_index = index + 1;
            for child in children {
                reset_abandoned(child, child_index, old, new, cursors);
                child_index += span(child);
            }
        }
    }
}
