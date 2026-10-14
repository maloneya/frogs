//! Bodies whose motion is one joint coordinate, held by a breakable lock.
//!
//! A hinge turns a box about a vertical axis at its local negative-X edge,
//! between angular stops. A topple joint stands a pole on its base; unlocked,
//! it tips about a horizontal axis at the base under gravity, out of the ground
//! plane, until it lies flat. The ground plane has no height, so a pole
//! declares the height at which ground-plane contacts and impacts meet it.
//!
//! A lock holds the coordinate. Contacts and impulses load it rather than
//! moving the body; once a tick's load exceeds its strength it fails having
//! spent exactly that strength, and the excess becomes the joint's motion in
//! the same substep. Loading does not accumulate across ticks. Strength and
//! reaction are linear impulse at the joint's reference arm: a hinge's free
//! edge, a pole's contact height.
use crate::{Alpha, EntityId, Fnv, Wall};
use arpg_core::Report;
use core::f32::consts::{FRAC_PI_2, PI};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Maximum live joints; independent of crowd population.
pub const MAX_JOINTS: usize = 192;
const GRAVITY: f32 = 9.81;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HingeInput {
    width: f32,
    thickness: f32,
    closed_yaw: f32,
    min_angle: f32,
    max_angle: f32,
}
/// A box turning about its local negative-X edge. Angles are radians.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "HingeInput", into = "HingeInput")]
pub struct HingeConfig {
    width: f32,
    thickness: f32,
    closed_yaw: f32,
    min_angle: f32,
    max_angle: f32,
}
impl HingeConfig {
    /// A box whose rest angle, zero, lies between its stops.
    pub fn new(width: f32, thickness: f32, closed_yaw: f32, min_angle: f32, max_angle: f32) -> Result<Self, &'static str> {
        if ![width, thickness, closed_yaw, min_angle, max_angle].iter().all(|n| n.is_finite())
            || !(0.1..=20.0).contains(&width)
            || !(0.01..=2.0).contains(&thickness)
            || thickness > width
            || min_angle > 0.0
            || max_angle < 0.0
            || min_angle >= max_angle
            || min_angle < -PI
            || max_angle > PI
        {
            return Err(
                "hinge needs width 0.1..=20m, thickness 0.01..=2m <= width, finite yaw, and ordered stops containing zero within ±pi",
            );
        }
        Ok(Self { width, thickness, closed_yaw: crate::angle::wrap(closed_yaw), min_angle, max_angle })
    }
    /// Full box width in metres; the free edge is this far from the hinge.
    pub fn width(self) -> f32 {
        self.width
    }
    /// Full box thickness in metres.
    pub fn thickness(self) -> f32 {
        self.thickness
    }
    /// Rest orientation, radians about world Y.
    pub fn closed_yaw(self) -> f32 {
        self.closed_yaw
    }
    /// Lower permitted angle from rest.
    pub fn min_angle(self) -> f32 {
        self.min_angle
    }
    /// Upper permitted angle from rest.
    pub fn max_angle(self) -> f32 {
        self.max_angle
    }
    fn hinge(self, centre: Vec2) -> Vec2 {
        centre - crate::angle::axis(self.closed_yaw) * self.width * 0.5
    }
    /// Distance from the hinge to the farthest corner.
    fn reach(self) -> f32 {
        (self.width * self.width + (self.thickness * 0.5) * (self.thickness * 0.5)).sqrt()
    }
}
impl TryFrom<HingeInput> for HingeConfig {
    type Error = &'static str;
    fn try_from(v: HingeInput) -> Result<Self, Self::Error> {
        Self::new(v.width, v.thickness, v.closed_yaw, v.min_angle, v.max_angle)
    }
}
impl From<HingeConfig> for HingeInput {
    fn from(v: HingeConfig) -> Self {
        Self { width: v.width, thickness: v.thickness, closed_yaw: v.closed_yaw, min_angle: v.min_angle, max_angle: v.max_angle }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToppleInput {
    width: f32,
    height: f32,
    yaw: f32,
    contact_height: f32,
    sweep: f32,
}
/// A pole standing on a square base. Lengths are metres.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "ToppleInput", into = "ToppleInput")]
pub struct ToppleConfig {
    width: f32,
    height: f32,
    yaw: f32,
    contact_height: f32,
    sweep: f32,
}
impl ToppleConfig {
    /// `width` is the square base; `contact_height` is where ground-plane
    /// contacts act and how tall the bodies it falls on are; `sweep` is the
    /// half-width of the swath it meets bodies in as it comes down.
    pub fn new(width: f32, height: f32, yaw: f32, contact_height: f32, sweep: f32) -> Result<Self, &'static str> {
        if ![width, height, yaw, contact_height, sweep].iter().all(|n| n.is_finite())
            || !(0.05..=2.0).contains(&width)
            || height > 20.0
            || height < 2.0 * width
            || contact_height <= 0.0
            || contact_height >= height
            || sweep < 0.5 * width
            || sweep > height
        {
            return Err(
                "topple joint needs width 0.05..=2m, height up to 20m and at least twice the width, contact height inside the height, and sweep between half the width and the height",
            );
        }
        Ok(Self { width, height, yaw: crate::angle::wrap(yaw), contact_height, sweep })
    }
    /// Side of the square base in metres.
    pub fn width(self) -> f32 {
        self.width
    }
    /// Base-to-tip length in metres.
    pub fn height(self) -> f32 {
        self.height
    }
    /// Base orientation, radians about world Y.
    pub fn yaw(self) -> f32 {
        self.yaw
    }
    /// Height at which ground-plane contacts and impacts meet the pole.
    pub fn contact_height(self) -> f32 {
        self.contact_height
    }
    /// Half-width of the swath a fall meets bodies in.
    pub fn sweep(self) -> f32 {
        self.sweep
    }
    fn base(self, position: Vec2) -> Result<Wall, &'static str> {
        Wall::new(position, Vec2::splat(self.width), self.yaw)
    }
    /// Ground-plane span the pole covers beyond its base at `tilt` toward
    /// `direction`, or None while too short to be a rectangle. Leaving the arena is Err.
    pub(crate) fn span(self, anchor: Vec2, direction: Vec2, tilt: f32) -> Result<Option<Wall>, ()> {
        let near = 0.5 * self.width;
        let length = self.height * tilt.sin() - near;
        if length < 0.01 {
            return Ok(None);
        }
        strip(anchor + direction * (near + 0.5 * length), self.width, length, direction).map(Some)
    }
    /// The pole lying toward `direction`, from half a base behind its pivot to its tip.
    pub(crate) fn lying(self, anchor: Vec2, direction: Vec2) -> Result<Wall, ()> {
        let back = 0.5 * self.width;
        let length = self.height + back;
        strip(anchor + direction * (0.5 * length - back), self.width, length, direction)
    }
}
impl TryFrom<ToppleInput> for ToppleConfig {
    type Error = &'static str;
    fn try_from(v: ToppleInput) -> Result<Self, Self::Error> {
        Self::new(v.width, v.height, v.yaw, v.contact_height, v.sweep)
    }
}
impl From<ToppleConfig> for ToppleInput {
    fn from(v: ToppleConfig) -> Self {
        Self { width: v.width, height: v.height, yaw: v.yaw, contact_height: v.contact_height, sweep: v.sweep }
    }
}

/// The single coordinate a jointed body moves along.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub enum JointConfig {
    /// Turns in the ground plane about a vertical axis.
    Hinge(HingeConfig),
    /// Tips out of the ground plane about a horizontal axis at its base.
    Topple(ToppleConfig),
}
impl JointConfig {
    /// Lever at which lock strength and reaction are measured.
    pub fn arm(self) -> f32 {
        match self {
            Self::Hinge(h) => h.width,
            Self::Topple(t) => t.contact_height,
        }
    }
    /// Farthest point from the pivot; converts joint rate into point speed.
    fn extent(self) -> f32 {
        match self {
            Self::Hinge(h) => h.reach(),
            Self::Topple(t) => t.height,
        }
    }
    /// Whether the coordinate turns the body within the ground plane.
    pub(crate) fn planar(self) -> bool {
        matches!(self, Self::Hinge(_))
    }
    /// Load at the arm from a ground-plane impulse at `r` from the pivot.
    pub(crate) fn at_arm(self, r: Vec2, impulse: Vec2) -> Vec2 {
        match self {
            Self::Hinge(h) => Vec2::new(lever(r, impulse) / h.width, 0.0),
            Self::Topple(_) => impulse,
        }
    }
    /// Ground-plane velocity at `r` from the pivot while the coordinate changes at `rate`.
    pub(crate) fn velocity_at(self, r: Vec2, rate: f32) -> Vec2 {
        if self.planar() { spin_velocity(r, rate) } else { Vec2::ZERO }
    }
    /// Coordinate rate added by `along`, impulse at the arm along the free direction.
    pub(crate) fn rate_from(self, along: f32, inverse_inertia: f32) -> f32 {
        along * self.arm() * inverse_inertia
    }
    /// Placement admission for a body at `position`. A hinge's complete
    /// swing must fit the arena. A pole's lying length is checked by its fall;
    /// what reaches behind the pivot fits within a disc of its base width.
    pub fn fits_at(self, position: Vec2) -> bool {
        match self {
            Self::Hinge(h) => (h.hinge(position).abs() + Vec2::splat(h.reach())).max_element() <= crate::ARENA_HALF,
            Self::Topple(t) => {
                t.base(position).is_ok()
                    && (position.abs() + Vec2::splat(t.width)).max_element() <= crate::ARENA_HALF
            }
        }
    }
}

/// Validated lock strength: impulse in kg m/s at the joint's arm over one tick.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct LockStrength(f32);
impl LockStrength {
    /// Read the admitted threshold.
    pub fn impulse(self) -> f32 {
        self.0
    }
}
impl TryFrom<f32> for LockStrength {
    type Error = &'static str;
    fn try_from(value: f32) -> Result<Self, Self::Error> {
        if !value.is_finite() || value <= 0.0 {
            return Err("lock strength must be positive and finite");
        }
        Ok(Self(value))
    }
}
impl From<LockStrength> for f32 {
    fn from(lock: LockStrength) -> Self {
        lock.0
    }
}

/// A locked joint bound to one explicit scene body.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JointSpec {
    /// Index into the scene's explicit body placements.
    pub body: usize,
    /// Validated geometry and limits.
    pub joint: JointConfig,
    /// Initial lock.
    pub lock: LockStrength,
}

/// Lock state, independent of how far the joint has moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum LockState {
    /// Intact and holding.
    Held,
    /// Removed by an explicit command.
    Released,
    /// Failed under physical load.
    Broken,
}

/// Immutable current mechanics of one jointed body.
#[derive(Clone, Copy, Debug)]
pub struct JointState {
    pub(crate) id: EntityId,
    pub(crate) kind: Kind,
    pub(crate) anchor: Vec2,
    /// Hinge angle from rest, or pole tilt from vertical.
    pub(crate) angle: f32,
    pub(crate) previous: f32,
    pub(crate) rate: f32,
    pub(crate) inverse_inertia: f32,
    pub(crate) lock: LockState,
    pub(crate) strength: f32,
    pub(crate) reaction: f32,
    /// Direct impulses since the last solve, at the arm, absorbed by the next one.
    pub(crate) pending: Vec2,
}

/// Geometry and limits, with the working state only that kind of joint has.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    Hinge {
        config: HingeConfig,
        /// Warm-start torques (N m) for the two angular stops.
        lower: f32,
        upper: f32,
    },
    Topple {
        config: ToppleConfig,
        /// Unit direction the pole tips toward; zero until a load picks it.
        direction: Vec2,
        /// Ground-plane impulse its unlocked base absorbed this tick.
        load: Vec2,
    },
}

impl JointState {
    /// Stable body identity.
    pub fn id(self) -> EntityId {
        self.id
    }
    /// Geometry and limits.
    pub fn config(self) -> JointConfig {
        match self.kind {
            Kind::Hinge { config, .. } => JointConfig::Hinge(config),
            Kind::Topple { config, .. } => JointConfig::Topple(config),
        }
    }
    /// Fixed pivot on the ground plane: the hinge, or the pole's base centre.
    pub fn anchor(self) -> Vec2 {
        self.anchor
    }
    /// Hinge angle from rest, or pole tilt from vertical, in radians.
    pub fn angle(self) -> f32 {
        self.angle
    }
    /// Rate of the joint coordinate in radians per second.
    pub fn rate(self) -> f32 {
        self.rate
    }
    /// Unit direction a pole tips toward; zero for hinges and poles no load has tipped.
    pub fn direction(self) -> Vec2 {
        match self.kind {
            Kind::Topple { direction, .. } => direction,
            Kind::Hinge { .. } => Vec2::ZERO,
        }
    }
    /// Yaw that points local +Z along a pole's fall, once a load has picked it.
    pub fn heading(self) -> Option<f32> {
        let direction = self.direction();
        (direction != Vec2::ZERO).then(|| crate::angle::yaw_of(direction))
    }
    /// Whether a pole lies at its limit, flat on the ground.
    pub fn grounded(self) -> bool {
        matches!(self.kind, Kind::Topple { .. }) && self.angle == FRAC_PI_2
    }
    /// Current lock state.
    pub fn lock(self) -> LockState {
        self.lock
    }
    /// Most recent tick's lock reaction at the arm. A failing lock reports the
    /// strength it spent before failing.
    pub fn reaction_impulse(self) -> f32 {
        self.reaction
    }
    /// Current ground-plane collider. A pole keeps its base until it lies flat.
    pub fn collider(self) -> Wall {
        match self.kind {
            Kind::Hinge { config: h, .. } => Wall::new(
                self.anchor + crate::angle::axis(h.closed_yaw + self.angle) * h.width * 0.5,
                Vec2::new(h.width, h.thickness),
                h.closed_yaw + self.angle,
            )
            .expect("hinge swing admitted inside arena"),
            Kind::Topple { config: t, direction, .. } if self.grounded() => {
                t.lying(self.anchor, direction).expect("a pole lands only inside the arena")
            }
            Kind::Topple { config: t, .. } => t.base(self.anchor).expect("pole base admitted inside arena"),
        }
    }
    pub(crate) fn velocity_at(self, point: Vec2) -> Vec2 {
        self.config().velocity_at(point - self.anchor, self.rate)
    }
    /// Impulse at the arm that a world-space impulse delivers to the joint.
    pub(crate) fn at_arm(self, point: Vec2, impulse: Vec2) -> Vec2 {
        self.config().at_arm(point - self.anchor, impulse)
    }
    /// Fastest point speed this joint rate would give.
    pub(crate) fn point_speed(self, rate: f32) -> f32 {
        rate.abs() * self.config().extent()
    }
    /// Joint rate from an impulse at the arm along the joint's free direction.
    /// A pole's direction comes from its load, so one without a direction yet
    /// takes the whole impulse.
    pub(crate) fn rate_from(self, at_arm: Vec2) -> f32 {
        let along = match self.kind {
            Kind::Hinge { .. } => at_arm.x,
            Kind::Topple { direction, .. } if direction == Vec2::ZERO => at_arm.length(),
            Kind::Topple { direction, .. } => at_arm.dot(direction),
        };
        self.config().rate_from(along, self.inverse_inertia)
    }
    /// Impulse at the arm along the free direction that stopping `rate` takes.
    pub(crate) fn momentum(self, rate: f32) -> f32 {
        rate / (self.config().arm() * self.inverse_inertia)
    }
    /// An unlocked pole's base contacts still act at the contact height while
    /// that height stands over the base; beyond that the pivot takes them.
    pub(crate) fn base_turns(self) -> bool {
        match self.kind {
            Kind::Topple { config: t, .. } => t.contact_height * self.angle.tan() <= 0.5 * t.width,
            Kind::Hinge { .. } => false,
        }
    }
    pub(crate) fn at(mut self, alpha: Alpha) -> Self {
        self.angle = self.previous + (self.angle - self.previous) * alpha.get();
        self
    }
}
fn strip(centre: Vec2, width: f32, length: f32, direction: Vec2) -> Result<Wall, ()> {
    Wall::new(centre, Vec2::new(width, length), crate::angle::yaw_of(direction)).map_err(|_| ())
}
/// Angular acceleration of an unlocked pole under gravity.
pub(crate) fn gravity(t: ToppleConfig, tilt: f32) -> f32 {
    1.5 * GRAVITY * tilt.sin() / t.height
}
/// Angular lever of a unit impulse along `normal` applied at `r` from a pivot.
pub(crate) fn lever(r: Vec2, normal: Vec2) -> f32 {
    r.y * normal.x - r.x * normal.y
}
/// Velocity of the point `r` from a vertical pivot turning at `omega`.
pub(crate) fn spin_velocity(r: Vec2, omega: f32) -> Vec2 {
    Vec2::new(r.y, -r.x) * omega
}

/// Interpolated joint pose for rendering.
pub type JointPresentation = JointState;

/// A moving jointed body met another body during the latest tick. What the
/// meeting means is a gameplay decision; this records the physics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub(crate) source: EntityId,
    pub(crate) body: EntityId,
    pub(crate) point: Vec2,
    pub(crate) closing_speed: f32,
    pub(crate) impulse: f32,
    pub(crate) damaged: bool,
}
impl Impact {
    /// The moving body.
    pub fn source(self) -> EntityId {
        self.source
    }
    /// The body it met.
    pub fn body(self) -> EntityId {
        self.body
    }
    /// The met body's position.
    pub fn point(self) -> Vec2 {
        self.point
    }
    /// Approach speed in m/s; negative when the body was already moving away faster.
    pub fn closing_speed(self) -> f32 {
        self.closing_speed
    }
    /// Momentum delivered to the met body, in kg m/s.
    pub fn impulse(self) -> f32 {
        self.impulse
    }
    /// Whether gameplay has already applied damage for this impact.
    pub fn damaged(self) -> bool {
        self.damaged
    }
}

#[derive(Default)]
pub(crate) struct Joints {
    who: crate::members::Members,
    pub(crate) rows: Vec<JointState>,
    /// Bodies each falling pole has met, aligned with `rows`; one impact per
    /// live body per fall. Revocation retires an identity from every history.
    pub(crate) met: Vec<Vec<EntityId>>,
    /// Lossless outcomes of the latest tick, replaced by the next step. Like
    /// trace events they keep bodies removed later, such as a crushed body's
    /// defeat; `damage_impact` refuses those.
    pub(crate) impacts: Vec<Impact>,
    /// Scratch rows near a fall; holds no identities between ticks.
    pub(crate) candidates: Vec<u32>,
}
impl Joints {
    pub(crate) fn get(&self, id: EntityId) -> Option<JointState> {
        self.index(id).map(|i| self.rows[i])
    }
    pub(crate) fn index(&self, id: EntityId) -> Option<usize> {
        self.who.index(id)
    }
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }
    pub(crate) fn install(&mut self, id: EntityId, position: Vec2, spec: JointSpec, inverse_mass: f32) {
        assert!(self.rows.len() < MAX_JOINTS && self.get(id).is_none() && inverse_mass > 0.0);
        let (kind, anchor, per_mass) = match spec.joint {
            JointConfig::Hinge(h) => (
                Kind::Hinge { config: h, lower: 0.0, upper: 0.0 },
                h.hinge(position),
                (4.0 * h.width * h.width + h.thickness * h.thickness) / 12.0,
            ),
            // A slender rod about its end.
            JointConfig::Topple(t) => (
                Kind::Topple { config: t, direction: Vec2::ZERO, load: Vec2::ZERO },
                position,
                t.height * t.height / 3.0,
            ),
        };
        self.who.add(id);
        self.rows.push(JointState {
            id,
            kind,
            anchor,
            angle: 0.0,
            previous: 0.0,
            rate: 0.0,
            inverse_inertia: inverse_mass / per_mass,
            lock: LockState::Held,
            strength: spec.lock.impulse(),
            reaction: 0.0,
            pending: Vec2::ZERO,
        });
        self.met.push(Vec::new());
    }
    pub(crate) fn revoke(&mut self, id: EntityId) {
        if let Some(i) = self.who.remove(id) {
            self.rows.swap_remove(i);
            self.met.swap_remove(i);
        }
        // A blocked fall can last indefinitely while crowd slots are reused.
        // Dead generations can never be met again; retaining them would let
        // this history grow beyond the bounded live population.
        for met in &mut self.met {
            met.retain(|&body| body != id);
        }
    }
    pub(crate) fn release(&mut self, id: EntityId) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        let joint = &mut self.rows[i];
        if joint.lock != LockState::Held {
            return false;
        }
        joint.lock = LockState::Released;
        // Impulses already admitted this tick were load on the lock. A free
        // hinge takes impulses as spin; a pole's next solve passes them on as
        // its tipping load, so one released without load stands balanced.
        if joint.config().planar() {
            joint.rate += joint.rate_from(joint.pending);
            joint.pending = Vec2::ZERO;
        }
        true
    }
    pub(crate) fn presentations(&self, alpha: Alpha) -> impl Iterator<Item = JointPresentation> + '_ {
        self.rows.iter().map(move |j| j.at(alpha))
    }
    pub(crate) fn finite(&self) -> bool {
        self.rows.iter().all(|j| {
            let kind = match j.kind {
                Kind::Hinge { lower, upper, .. } => lower.is_finite() && upper.is_finite(),
                Kind::Topple { load, .. } => load.is_finite(),
            };
            kind && j.angle.is_finite() && j.rate.is_finite() && j.reaction.is_finite() && j.pending.is_finite()
        })
    }
    pub(crate) fn hash(&self, h: &mut Fnv) {
        h.usize(self.rows.len());
        for (j, met) in self.rows.iter().zip(&self.met) {
            j.id.hash_into(h);
            match j.kind {
                Kind::Hinge { config: c, lower, upper } => {
                    h.usize(0);
                    for n in [c.width, c.thickness, c.closed_yaw, c.min_angle, c.max_angle, lower, upper] {
                        h.f32(n);
                    }
                }
                Kind::Topple { config: c, direction, load } => {
                    h.usize(1);
                    for n in [c.width, c.height, c.yaw, c.contact_height, c.sweep, direction.x, direction.y, load.x, load.y] {
                        h.f32(n);
                    }
                }
            }
            h.usize(j.lock as usize);
            for n in [
                j.anchor.x,
                j.anchor.y,
                j.angle,
                j.previous,
                j.rate,
                j.inverse_inertia,
                j.strength,
                j.reaction,
                j.pending.x,
                j.pending.y,
            ] {
                h.f32(n);
            }
            h.usize(met.len());
            for id in met {
                id.hash_into(h);
            }
        }
        h.usize(self.impacts.len());
        for i in &self.impacts {
            i.source.hash_into(h);
            i.body.hash_into(h);
            for n in [i.point.x, i.point.y, i.closing_speed, i.impulse] {
                h.f32(n);
            }
            h.usize(usize::from(i.damaged));
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("joints", |out| {
            for (j, met) in self.rows.iter().zip(&self.met) {
                out.object(&j.id.to_string(), |out| {
                    match j.kind {
                        Kind::Hinge { config: c, lower, upper } => {
                            out.text("kind", "Hinge");
                            out.num("width", c.width);
                            out.num("thickness", c.thickness);
                            out.num("closed_yaw", c.closed_yaw);
                            out.num("min_angle", c.min_angle);
                            out.num("max_angle", c.max_angle);
                            out.num("lower_stop_torque", lower);
                            out.num("upper_stop_torque", upper);
                        }
                        Kind::Topple { config: c, direction, load } => {
                            out.text("kind", "Topple");
                            out.num("width", c.width);
                            out.num("height", c.height);
                            out.num("yaw", c.yaw);
                            out.num("contact_height", c.contact_height);
                            out.num("sweep", c.sweep);
                            out.vec3("direction", crate::on_ground(direction, 0.0));
                            out.vec3("load", crate::on_ground(load, 0.0));
                            out.int("met", met.len() as u64);
                        }
                    }
                    out.vec3("anchor", crate::on_ground(j.anchor, 0.0));
                    let collider = j.collider();
                    out.vec3("collider_centre", crate::on_ground(collider.centre(), 0.0));
                    out.num("collider_width", collider.size().x);
                    out.num("collider_length", collider.size().y);
                    out.num("collider_yaw", collider.yaw());
                    out.num("inverse_inertia", j.inverse_inertia);
                    out.num("previous_angle", j.previous);
                    out.num("angle", j.angle);
                    out.num("rate", j.rate);
                    out.text("lock", &format!("{:?}", j.lock));
                    out.num("lock_strength", j.strength);
                    out.num("reaction_impulse", j.reaction);
                    out.vec3("pending_impulse", crate::on_ground(j.pending, 0.0));
                });
            }
        });
        out.object("impacts", |out| {
            for (n, i) in self.impacts.iter().enumerate() {
                out.object(&n.to_string(), |out| {
                    out.text("source", &i.source.to_string());
                    out.text("body", &i.body.to_string());
                    out.vec3("point", crate::on_ground(i.point, 0.0));
                    out.num("closing_speed", i.closing_speed);
                    out.num("impulse", i.impulse);
                    out.bool("damaged", i.damaged);
                });
            }
        });
    }
}
