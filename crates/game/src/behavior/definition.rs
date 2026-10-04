//! Authored, bounded action composition. Admission validates the complete definition.

use arpg_sim::Fnv;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

/// Maximum operations available to one shared definition.
pub const MAX_ACTIONS: usize = 16;
/// Maximum nodes in one complete behaviour composition.
pub const MAX_BEHAVIOR_NODES: usize = 64;
/// Maximum depth, counting the root, of a behaviour composition.
pub const MAX_BEHAVIOR_DEPTH: usize = 8;

/// Gameplay classification, independent of physical movement and health.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum ActorRole {
    /// Counts towards hostile population gates and is eligible for player attack damage.
    Hostile,
    /// A character that does not count as a hostile or acquire hostility implicitly.
    #[default]
    Civilian,
    /// An object rather than a character; it can still expose an interaction.
    Prop,
}

/// A bounded query used when beginning an action, never a retained dense row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum TargetSelection {
    /// The player. Combat admission additionally requires a hostile acting role.
    #[default]
    Player,
    /// Nearest eligible hostile, breaking equal distances by stable identity.
    NearestHostile,
    /// Nearest Ready interaction endpoint, excluding the acting entity.
    NearestInteraction,
}

/// Movement can follow a selected entity or approach an authored world point.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub enum Destination {
    /// Reobserve a stable eligible target while moving.
    Target(TargetSelection),
    /// World ground coordinates `(x, z)`.
    Point((f32, f32)),
}

/// Settings for an interruptible movement action.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MoveAction {
    /// Desired world point or target selection.
    pub destination: Destination,
    /// Maximum desired ground speed in metres per second.
    pub speed: f32,
    /// Bounded motor acceleration in metres per second squared.
    pub acceleration: f32,
    /// Centre-to-centre arrival distance in metres.
    pub stop_distance: f32,
}
impl Default for MoveAction {
    fn default() -> Self {
        Self {
            destination: Destination::Target(TargetSelection::Player),
            speed: 2.5,
            acceleration: 30.0,
            stop_distance: arpg_sim::PLAYER_RADIUS + arpg_sim::ENEMY_RADIUS,
        }
    }
}

/// Completed-tick windows captured when a committed action starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionTiming {
    /// Warning ticks before any active effect.
    pub startup: u32,
    /// Active strike or travel ticks.
    pub active: u32,
    /// Recovery ticks before another action can start.
    pub recovery: u32,
}
impl ActionTiming {
    fn valid(self) -> bool {
        self.startup > 0
            && self.active > 0
            && self.recovery > 0
            && self.startup <= 3600
            && self.active <= 3600
            && self.recovery <= 3600
            && arpg_sim::ActionSchedule::timed(self.startup, self.active, self.recovery).is_ok()
    }
    fn hash(self, out: &mut Fnv) {
        out.usize(self.startup as usize);
        out.usize(self.active as usize);
        out.usize(self.recovery as usize);
    }
}

/// A direction-committed close strike, independent of its selecting behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SwipeAction {
    /// Query used at action admission.
    pub target: TargetSelection,
    /// Windup, active and recovery windows.
    pub timing: ActionTiming,
    /// Maximum centre-to-centre distance at admission.
    pub reach: f32,
    /// Hit disc distance along the committed direction.
    pub offset: f32,
    /// Hit disc radius, excluding the target's physical radius.
    pub radius: f32,
    /// Positive damage submitted at most once to the committed target.
    pub damage: NonZeroU8,
    /// Momentum delivered to each contacted body, independent of health or allegiance.
    pub impulse: f32,
}
impl Default for SwipeAction {
    fn default() -> Self {
        Self {
            target: TargetSelection::Player,
            timing: ActionTiming {
                startup: 36,
                active: 9,
                recovery: 39,
            },
            reach: 1.5,
            offset: 0.75,
            radius: 0.65,
            damage: NonZeroU8::new(1).expect("positive damage"),
            impulse: 1.0,
        }
    }
}

/// A committed landing point reached through the ordinary physical motor.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeapAction {
    /// Query used at action admission.
    pub target: TargetSelection,
    /// Windup, travel and recovery windows.
    pub timing: ActionTiming,
    /// Minimum centre-to-centre admission distance.
    pub min_range: f32,
    /// Maximum centre-to-centre admission distance.
    pub max_range: f32,
    /// Landing disc radius, excluding target collision radius.
    pub radius: f32,
    /// Ticks between starts of this particular action.
    pub cooldown: u32,
    /// Maximum desired flight speed in metres per second.
    pub speed: f32,
    /// Flight motor acceleration in metres per second squared.
    pub acceleration: f32,
    /// Positive landing damage.
    pub damage: NonZeroU8,
    /// Momentum delivered to each contacted body, independent of health or allegiance.
    pub impulse: f32,
}
impl Default for LeapAction {
    fn default() -> Self {
        Self {
            target: TargetSelection::Player,
            timing: ActionTiming {
                startup: 42,
                active: 36,
                recovery: 54,
            },
            min_range: 3.0,
            max_range: 7.0,
            radius: 0.9,
            cooldown: 240,
            speed: 18.0,
            acceleration: 120.0,
            damage: NonZeroU8::new(1).expect("positive damage"),
            impulse: 1.0,
        }
    }
}

/// Operations available to this definition. A tree can only name these entries.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum ActionDefinition {
    /// Approach a target or point using shared physics.
    Move(MoveAction),
    /// Hold position for a bounded number of ticks.
    Wait {
        /// Positive duration in simulation ticks.
        ticks: u32,
    },
    /// Activate an eligible nearby Ready endpoint.
    Interact {
        /// The endpoint query; only NearestInteraction is admissible.
        target: TargetSelection,
    },
    /// Telegraph and execute a direction-committed strike.
    Swipe(SwipeAction),
    /// Telegraph, travel and resolve a single landing effect.
    Leap(LeapAction),
}

/// A small typed composition, with no runtime scripts or ambient randomness.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum BehaviorNode {
    /// Produce no action.
    #[default]
    Idle,
    /// Execute an index from the definition's action list.
    Action(usize),
    /// Complete children in order; failure resets the sequence.
    Sequence(Vec<Self>),
    /// Select the first admissible child; committed actions retain control.
    Priority(Vec<Self>),
}

/// Complete spawn-time actor definition, shared immutably by its instances.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActorDefinition {
    /// Physical body properties; gameplay grants are validated separately.
    pub physical: arpg_sim::Template,
    /// Explicit gameplay/presentation role.
    pub role: ActorRole,
    /// Available operations, referenced by the composition below.
    pub actions: Vec<ActionDefinition>,
    /// Decision composition shared by all instances of this definition.
    pub behavior: BehaviorNode,
}
impl Default for ActorDefinition {
    fn default() -> Self {
        Self {
            physical: arpg_sim::Template::BODY,
            role: ActorRole::Civilian,
            actions: Vec::new(),
            behavior: BehaviorNode::Idle,
        }
    }
}
impl ActorDefinition {
    /// A stationary hostile character with three health points and no actions.
    #[must_use]
    pub fn passive_hostile() -> Self {
        Self {
            physical: arpg_sim::Template::BODY
                .character()
                .with_health(NonZeroU8::new(3).expect("positive health")),
            role: ActorRole::Hostile,
            ..Self::default()
        }
    }
    /// A pursuing hostile with no attack capabilities.
    #[must_use]
    pub fn hostile() -> Self {
        Self {
            actions: vec![ActionDefinition::Move(MoveAction::default())],
            behavior: BehaviorNode::Action(0),
            ..Self::passive_hostile()
        }
    }
    /// A hostile that chooses close swipes, mid-range leaps, then pursuit.
    #[must_use]
    pub fn attacker() -> Self {
        Self {
            actions: vec![
                ActionDefinition::Swipe(SwipeAction::default()),
                ActionDefinition::Leap(LeapAction::default()),
                ActionDefinition::Move(MoveAction::default()),
            ],
            behavior: BehaviorNode::Priority(vec![
                BehaviorNode::Action(0),
                BehaviorNode::Action(1),
                BehaviorNode::Action(2),
            ]),
            ..Self::passive_hostile()
        }
    }
    /// A passive nonhostile character; author actions to give it a routine.
    #[must_use]
    pub fn civilian() -> Self {
        Self {
            physical: arpg_sim::Template::BODY.character(),
            ..Self::default()
        }
    }
    /// A fixed, immune object. Interaction is an independent physical grant.
    #[must_use]
    pub fn prop() -> Self {
        Self {
            physical: arpg_sim::Template::BLOCK,
            role: ActorRole::Prop,
            ..Self::default()
        }
    }
    /// Expose an interaction endpoint independently of role, health or behaviour.
    #[must_use]
    pub fn interactive(mut self) -> Self {
        self.physical = self.physical.interactive();
        self
    }
    /// Refuse invalid capabilities, references and tuning before any spawn mutation.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.actions.len() > MAX_ACTIONS {
            return Err("actor exceeds action capacity");
        }
        for action in &self.actions {
            match action {
                ActionDefinition::Move(settings) => {
                    if self.physical.fixed() {
                        return Err("movement requires a movable body");
                    }
                    arpg_sim::MotorCommand::validate(
                        glam::Vec2::X * settings.speed,
                        settings.acceleration,
                    )?;
                    if settings.speed <= 0.0
                        || settings.acceleration <= 0.0
                        || !nonnegative(settings.stop_distance, 1024.0)
                    {
                        return Err("invalid movement settings");
                    }
                    match settings.destination {
                        Destination::Point(point)
                            if !arpg_sim::valid_position(glam::Vec2::from(point)) =>
                        {
                            return Err("invalid movement destination");
                        }
                        Destination::Target(_) => {}
                        Destination::Point(_) => {}
                    }
                }
                ActionDefinition::Wait { ticks } => {
                    if *ticks == 0 || *ticks > 216000 {
                        return Err("invalid wait duration");
                    }
                }
                ActionDefinition::Interact { target } => {
                    if *target != TargetSelection::NearestInteraction {
                        return Err("interaction requires an interaction target");
                    }
                }
                ActionDefinition::Swipe(settings) => {
                    self.validate_target(settings.target)?;
                    if settings.target == TargetSelection::NearestInteraction {
                        return Err("a combat action requires a combat target");
                    }
                    if !valid_impulse(settings.impulse) || !settings.timing.valid()
                        || !positive(settings.reach, 64.0)
                        || !nonnegative(settings.offset, 64.0)
                        || !positive(settings.radius, 64.0)
                    {
                        return Err("invalid swipe settings");
                    }
                }
                ActionDefinition::Leap(settings) => {
                    self.validate_target(settings.target)?;
                    if self.physical.fixed() {
                        return Err("leap requires a movable body");
                    }
                    if settings.target == TargetSelection::NearestInteraction {
                        return Err("a combat action requires a combat target");
                    }
                    arpg_sim::MotorCommand::validate(
                        glam::Vec2::X * settings.speed,
                        settings.acceleration,
                    )?;
                    if !valid_impulse(settings.impulse) || !settings.timing.valid()
                        || !nonnegative(settings.min_range, 64.0)
                        || !positive(settings.max_range, 64.0)
                        || settings.min_range > settings.max_range
                        || !positive(settings.radius, 64.0)
                        || settings.speed <= 0.0
                        || settings.acceleration <= 0.0
                        || settings.cooldown > 216000
                    {
                        return Err("invalid leap settings");
                    }
                }
            }
        }
        self.behavior.validate(self.actions.len(), 0, &mut 0)
    }
    fn validate_target(&self, target: TargetSelection) -> Result<(), &'static str> {
        if target == TargetSelection::Player && self.role != ActorRole::Hostile {
            return Err("only hostile definitions can target the player");
        }
        Ok(())
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        self.physical.hash_into(out);
        out.usize(self.role as usize);
        out.usize(self.actions.len());
        for action in &self.actions {
            action.hash(out);
        }
        self.behavior.hash(out);
    }
    /// Complete immutable configuration for diagnostics and authoring inspection.
    pub(crate) fn report(&self, out: &mut arpg_core::Report) {
        out.text(
            "role",
            match self.role {
                ActorRole::Hostile => "hostile",
                ActorRole::Civilian => "civilian",
                ActorRole::Prop => "prop",
            },
        );
        out.bool("fixed", self.physical.fixed());
        out.bool("character", self.physical.is_character());
        out.bool("interactable", self.physical.interactable());
        out.int(
            "initial_health",
            self.physical
                .hit_points()
                .map_or(0, |health| u64::from(health.get())),
        );
        out.object("actions", |out| {
            for (index, action) in self.actions.iter().enumerate() {
                out.object(&index.to_string(), |out| action.report(out));
            }
        });
        out.object("behavior", |out| self.behavior.report(out));
    }
}
fn positive(value: f32, limit: f32) -> bool {
    value.is_finite() && value > 0.0 && value <= limit
}
fn nonnegative(value: f32, limit: f32) -> bool {
    value.is_finite() && value >= 0.0 && value <= limit
}

impl BehaviorNode {
    fn validate(
        &self,
        actions: usize,
        depth: usize,
        count: &mut usize,
    ) -> Result<(), &'static str> {
        *count += 1;
        if depth >= MAX_BEHAVIOR_DEPTH || *count > MAX_BEHAVIOR_NODES {
            return Err("behavior exceeds composition bounds");
        }
        match self {
            Self::Idle => Ok(()),
            Self::Action(index) => {
                if *index < actions {
                    Ok(())
                } else {
                    Err("behavior references an unavailable action")
                }
            }
            Self::Sequence(children) | Self::Priority(children) => {
                if children.is_empty() {
                    return Err("behavior composition cannot be empty");
                }
                for child in children {
                    child.validate(actions, depth + 1, count)?;
                }
                Ok(())
            }
        }
    }
    fn hash(&self, out: &mut Fnv) {
        match self {
            Self::Idle => out.usize(0),
            Self::Action(index) => {
                out.usize(1);
                out.usize(*index);
            }
            Self::Sequence(children) | Self::Priority(children) => {
                out.usize(if matches!(self, Self::Sequence(_)) {
                    2
                } else {
                    3
                });
                out.usize(children.len());
                for child in children {
                    child.hash(out);
                }
            }
        }
    }
    fn report(&self, out: &mut arpg_core::Report) {
        match self {
            Self::Idle => out.text("kind", "idle"),
            Self::Action(index) => {
                out.text("kind", "action");
                out.int("index", *index as u64);
            }
            Self::Sequence(children) | Self::Priority(children) => {
                out.text(
                    "kind",
                    if matches!(self, Self::Sequence(_)) {
                        "sequence"
                    } else {
                        "priority"
                    },
                );
                out.object("children", |out| {
                    for (index, child) in children.iter().enumerate() {
                        out.object(&index.to_string(), |out| child.report(out));
                    }
                });
            }
        }
    }
}
impl ActionDefinition {
    fn report(&self, out: &mut arpg_core::Report) {
        match self {
            Self::Move(settings) => {
                out.text("kind", "move");
                match settings.destination {
                    Destination::Target(target) => out.text("target", target.label()),
                    Destination::Point((x, z)) => {
                        out.num("x", x);
                        out.num("z", z);
                    }
                }
                out.num("speed", settings.speed);
                out.num("acceleration", settings.acceleration);
                out.num("stop_distance", settings.stop_distance);
            }
            Self::Wait { ticks } => {
                out.text("kind", "wait");
                out.int("ticks", u64::from(*ticks));
            }
            Self::Interact { target } => {
                out.text("kind", "interact");
                out.text("target", target.label());
            }
            Self::Swipe(settings) => {
                out.text("kind", "swipe");
                out.text("target", settings.target.label());
                settings.timing.report(out);
                out.num("reach", settings.reach);
                out.num("offset", settings.offset);
                out.num("radius", settings.radius);
                out.int("damage", u64::from(settings.damage.get()));
                out.num("impulse", settings.impulse);
            }
            Self::Leap(settings) => {
                out.text("kind", "leap");
                out.text("target", settings.target.label());
                settings.timing.report(out);
                out.num("min_range", settings.min_range);
                out.num("max_range", settings.max_range);
                out.num("radius", settings.radius);
                out.int("cooldown", u64::from(settings.cooldown));
                out.num("speed", settings.speed);
                out.num("acceleration", settings.acceleration);
                out.int("damage", u64::from(settings.damage.get()));
                out.num("impulse", settings.impulse);
            }
        }
    }
    fn hash(&self, out: &mut Fnv) {
        match self {
            Self::Move(settings) => {
                out.usize(0);
                match settings.destination {
                    Destination::Target(target) => {
                        out.usize(0);
                        out.usize(target as usize);
                    }
                    Destination::Point((x, z)) => {
                        out.usize(1);
                        out.f32(x);
                        out.f32(z);
                    }
                }
                out.f32(settings.speed);
                out.f32(settings.acceleration);
                out.f32(settings.stop_distance);
            }
            Self::Wait { ticks } => {
                out.usize(1);
                out.usize(*ticks as usize);
            }
            Self::Interact { target } => {
                out.usize(2);
                out.usize(*target as usize);
            }
            Self::Swipe(s) => {
                out.usize(3);
                out.usize(s.target as usize);
                s.timing.hash(out);
                out.f32(s.reach);
                out.f32(s.offset);
                out.f32(s.radius);
                out.usize(usize::from(s.damage.get()));
                out.f32(s.impulse);
            }
            Self::Leap(s) => {
                out.usize(4);
                out.usize(s.target as usize);
                s.timing.hash(out);
                out.f32(s.min_range);
                out.f32(s.max_range);
                out.f32(s.radius);
                out.usize(s.cooldown as usize);
                out.f32(s.speed);
                out.f32(s.acceleration);
                out.usize(usize::from(s.damage.get()));
                out.f32(s.impulse);
            }
        }
    }
}
impl TargetSelection {
    fn label(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::NearestHostile => "nearest_hostile",
            Self::NearestInteraction => "nearest_interaction",
        }
    }
}
impl ActionTiming {
    fn report(self, out: &mut arpg_core::Report) {
        out.int("startup", u64::from(self.startup));
        out.int("active", u64::from(self.active));
        out.int("recovery", u64::from(self.recovery));
    }
}

/// Reuse impulse admission without introducing a gameplay-specific magnitude cap.
fn valid_impulse(magnitude: f32) -> bool {
    magnitude >= 0.0 && arpg_sim::Impulse::try_from((magnitude, 0.0)).is_ok()
}
