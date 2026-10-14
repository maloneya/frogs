//! Doors: a scene body on a locked hinge. The hinge and lock are physics;
//! what a door is, and which lock state means what, belongs here.
use crate::encounter::EncounterEvent;
use arpg_core::Report;
use arpg_sim::{
    Alpha, EntityId, Fnv, GameplayAccess, HingeConfig, JointConfig, JointSpec, LockState,
    LockStrength, SceneId, TraceSink, Wall, World,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawPanel {
    width: f32,
    thickness: f32,
    closed_yaw: f32,
    min_angle: f32,
    max_angle: f32,
    latch_impulse: f32,
}

/// Door geometry, swing limits and latch strength, validated by the physical
/// hinge and lock they become. Angles are radians; latch strength is impulse
/// in kg m/s at the edge opposite the hinge, over one tick.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "RawPanel", into = "RawPanel")]
pub struct DoorPanel {
    hinge: HingeConfig,
    latch: LockStrength,
}
impl DoorPanel {
    /// A panel whose closed angle lies between its stops.
    pub fn new(width: f32, thickness: f32, closed_yaw: f32, min_angle: f32, max_angle: f32, latch_impulse: f32) -> Result<Self, &'static str> {
        Ok(Self {
            hinge: HingeConfig::new(width, thickness, closed_yaw, min_angle, max_angle)?,
            latch: LockStrength::try_from(latch_impulse)?,
        })
    }
    /// Panel geometry and swing limits; rest is the closed angle.
    pub fn hinge(self) -> HingeConfig { self.hinge }
    /// Latch failure threshold at the free edge.
    pub fn latch_impulse(self) -> f32 { self.latch.impulse() }
    pub(crate) fn joint(self, body: usize) -> JointSpec {
        JointSpec { body, joint: JointConfig::Hinge(self.hinge), lock: self.latch }
    }
}
impl Default for DoorPanel {
    fn default() -> Self {
        Self::new(2.0, 0.18, 0.0, -1.55, 1.55, 12.0).expect("valid door defaults")
    }
}
impl TryFrom<RawPanel> for DoorPanel {
    type Error = &'static str;
    fn try_from(v: RawPanel) -> Result<Self, Self::Error> {
        Self::new(v.width, v.thickness, v.closed_yaw, v.min_angle, v.max_angle, v.latch_impulse)
    }
}
impl From<DoorPanel> for RawPanel {
    fn from(v: DoorPanel) -> Self {
        let h = v.hinge;
        Self { width: h.width(), thickness: h.thickness(), closed_yaw: h.closed_yaw(), min_angle: h.min_angle(), max_angle: h.max_angle(), latch_impulse: v.latch_impulse() }
    }
}

/// A hinged panel attached to an explicit neutral body in this scene.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DoorSpec {
    /// Index in the scene's explicit bodies.
    pub body: usize,
    /// Validated physical shape, swing limits and latch strength.
    pub panel: DoorPanel,
}

/// Observation of a door's lock, never a second simulation state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum DoorState {
    /// The latch still holds the panel.
    Latched,
    /// An explicit mechanism released the latch without breaking it.
    Released,
    /// Physical load exceeded the latch strength.
    Broken,
    /// The physical body was removed; distinct from a legitimate breach.
    Missing,
}
impl From<LockState> for DoorState {
    fn from(state: LockState) -> Self {
        match state {
            LockState::Held => Self::Latched,
            LockState::Released => Self::Released,
            LockState::Broken => Self::Broken,
        }
    }
}

/// Interpolated door geometry and state for presentation.
#[derive(Clone, Copy, Debug)]
pub struct DoorPose {
    /// Panel box at its interpolated angle.
    pub collider: Wall,
    /// Latch observation.
    pub state: DoorState,
}

struct Door { owner: SceneId, body: EntityId }

#[derive(Default)]
pub(crate) struct Doors { rows: Vec<Door> }

impl Doors {
    pub(crate) fn accepts(actor: &crate::ActorDefinition) -> bool {
        !actor.physical.fixed() && !actor.physical.is_character()
            && actor.role == crate::ActorRole::Prop && actor.actions.is_empty()
    }
    /// Gameplay's rule only; simulation admission owns the body binding,
    /// uniqueness, capacity and swing checks for the joint itself.
    pub(crate) fn validate(specs: &[DoorSpec], bodies: &[crate::Placed]) -> Result<(), crate::SceneError> {
        if specs.iter().any(|door| bodies.get(door.body).is_some_and(|body| !Self::accepts(&body.what))) {
            return Err(crate::SceneError::Encounter("door must bind a movable prop without actor actions"));
        }
        Ok(())
    }
    pub(crate) fn install(&mut self, owner: SceneId, specs: &[DoorSpec], bodies: &[EntityId]) {
        self.rows.extend(specs.iter().map(|spec| Door { owner, body: bodies[spec.body] }));
    }
    pub(crate) fn evict(&mut self, owner: SceneId) { self.rows.retain(|row| row.owner != owner); }
    fn row(&self, owner: SceneId, nth: usize) -> Option<&Door> {
        self.rows.iter().filter(|row| row.owner == owner).nth(nth)
    }
    pub(crate) fn state(&self, owner: SceneId, nth: usize, world: &World) -> Option<DoorState> {
        self.row(owner, nth).map(|row| world.joint(row.body)
            .map_or(DoorState::Missing, |joint| joint.lock().into()))
    }
    pub(crate) fn poses<'a>(&'a self, world: &'a World, alpha: Alpha) -> impl Iterator<Item = DoorPose> + 'a {
        self.rows.iter().filter_map(move |row| {
            let joint = world.joint_presentation(row.body, alpha)?;
            Some(DoorPose { collider: joint.collider(), state: joint.lock().into() })
        })
    }
    pub(crate) fn available(&self, owner: SceneId, nth: usize, access: &GameplayAccess<'_>) -> bool {
        self.row(owner, nth).is_some_and(|row| access.joint(row.body).is_some())
    }
    pub(crate) fn unlock(&self, owner: SceneId, nth: usize, access: &mut GameplayAccess<'_>, trace: &mut TraceSink<'_, EncounterEvent>) {
        let Some(row) = self.row(owner, nth) else { return };
        if access.release_lock(row.body) {
            trace.emit(EncounterEvent::Door { scene: owner, index: nth, state: DoorState::Released });
        }
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for row in &self.rows { row.owner.hash_into(out); row.body.hash_into(out); }
    }
    pub(crate) fn report(&self, world: &World, out: &mut Report) {
        out.object("doors", |out| {
            for group in self.rows.chunk_by(|a, b| a.owner == b.owner) {
                for (nth, row) in group.iter().enumerate() {
                    out.object(&format!("{}/{nth}", row.owner), |out| {
                        out.text("body", &row.body.to_string());
                        out.text("state", &format!("{:?}", self.state(row.owner, nth, world).expect("resident door")));
                    });
                }
            }
        });
    }
}
