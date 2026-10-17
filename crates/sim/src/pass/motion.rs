//! Physical velocity: motors and impulses share motion and contact response.
//!
//! All actor locomotion uses bounded motors, so collisions can slow movement and
//! transmit running momentum. Overlap correction
//! is bounded by the contact push speed. Passive bodies damp after contacts; motors apply the same drag before
//! acceleration, then mark it consumed so settle cannot damp them twice.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::members::Members;
use crate::slots::Slots;
use crate::trace::{Event, TraceSink};
use crate::{Dt, EntityId, Fnv};
use arpg_core::Report;

/// Finite, positive body mass in kilograms, admitted identically by files and callers.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct Mass(f32);

impl Mass {
    /// Smallest supported dynamic body, avoiding unbounded inverse mass.
    pub const MIN: f32 = 0.1;
    /// Largest supported dynamic body for the shared solver's numeric range.
    pub const MAX: f32 = 10_000.0;
    /// Default movable prop and actor mass.
    pub const UNIT: Self = Self(1.0);
    /// Read the admitted mass in kilograms.
    #[must_use]
    pub const fn kilograms(self) -> f32 {
        self.0
    }
}
impl Default for Mass {
    fn default() -> Self {
        Self::UNIT
    }
}
impl TryFrom<f32> for Mass {
    type Error = &'static str;
    fn try_from(value: f32) -> Result<Self, Self::Error> {
        if !value.is_finite() || !(Self::MIN..=Self::MAX).contains(&value) {
            return Err("mass must be finite and in 0.1..=10000 kilograms");
        }
        Ok(Self(value))
    }
}
impl From<Mass> for f32 {
    fn from(mass: Mass) -> Self {
        mass.0
    }
}

/// Retain 90% of carried speed per simulation tick (60 Hz), after collisions.
/// Geometric decay gives a free body a total travel of v / 6 world units.
const RETAIN: f32 = 0.9;
/// Snap the invisible tail to rest, so a settled body really has zero velocity.
const REST_SPEED: f32 = 0.001;
const _: () = assert!(RETAIN > 0.0 && RETAIN < 1.0 && REST_SPEED > 0.0);

pub(crate) const MAX_MOTOR_SPEED: f32 = 30.0;
pub(crate) const MAX_MOTOR_ACCELERATION: f32 = 300.0;

// Numerical admission envelope, far above gameplay speeds: finite f32 values
// alone permit catastrophic cancellation when contact reactions stop motion.
// Bound resulting point speed, so mass, leverage and carried velocity all count.
const MAX_POINT_SPEED: f32 = 1_000_000.0;
fn admitted_point_speed_squared(speed_squared: f32) -> bool {
    (0.0..=MAX_POINT_SPEED * MAX_POINT_SPEED).contains(&speed_squared)
}

/// Validated tick-local request from the single action controlling a body.
#[derive(Clone, Copy, Debug)]
pub struct MotorCommand {
    pub(crate) actor: EntityId,
    pub(crate) velocity: Vec2,
    pub(crate) acceleration: f32,
}
impl MotorCommand {
    /// Maximum accepted target speed, in metres per second.
    pub const MAX_SPEED: f32 = MAX_MOTOR_SPEED;
    /// Maximum accepted motor acceleration, in metres per second squared.
    pub const MAX_ACCELERATION: f32 = MAX_MOTOR_ACCELERATION;
    /// Shared admission for action definitions and emitted commands.
    pub fn validate(velocity: Vec2, acceleration: f32) -> Result<(), &'static str> {
        if !velocity.is_finite()
            || velocity.length_squared()
                > MAX_MOTOR_SPEED * MAX_MOTOR_SPEED * (1.0 + 4.0 * f32::EPSILON)
            || !acceleration.is_finite()
            || !(0.0..=MAX_MOTOR_ACCELERATION).contains(&acceleration)
        {
            return Err("motor needs finite speed <= 30m/s and acceleration in 0..=300m/s²");
        }
        Ok(())
    }
    /// A bounded finite target velocity and acceleration in world units.
    pub fn new(actor: EntityId, velocity: Vec2, acceleration: f32) -> Result<Self, &'static str> {
        Self::validate(velocity, acceleration)?;
        // A validated scalar speed multiplied by a normalized direction can
        // round just above the cap. Admit only that numerical fringe, then
        // keep the actual request strictly inside the physical speed bound.
        let velocity = if velocity.length_squared() > MAX_MOTOR_SPEED * MAX_MOTOR_SPEED {
            velocity * ((MAX_MOTOR_SPEED * (1.0 - 2.0 * f32::EPSILON)) / velocity.length())
        } else {
            velocity
        };
        Ok(Self {
            actor,
            velocity,
            acceleration,
        })
    }
}

/// A finite world-ground-plane momentum change, shared by gameplay and readers.
/// Deserializing uses the same validation as an engine caller.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(try_from = "(f32, f32)")]
pub struct Impulse(Vec2);

impl Impulse {
    /// Read the momentum components in world X/Z for typed observations.
    #[must_use]
    pub fn components(self) -> (f32, f32) {
        (self.0.x, self.0.y)
    }
}

impl TryFrom<(f32, f32)> for Impulse {
    type Error = &'static str;

    fn try_from((x, z): (f32, f32)) -> Result<Self, Self::Error> {
        let value = Vec2::new(x, z);
        if !value.is_finite() || !value.length_squared().is_finite() {
            return Err("impulse must have a finite magnitude");
        }
        Ok(Self(value))
    }
}

impl core::fmt::Display for Impulse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "({:.4},{:.4})", self.0.x, self.0.y)
    }
}

/// A body's physical state. Fields are private: mass is validated at grant,
/// and a caller can inspect velocity without being able to replace it.
#[derive(Clone, Copy)]
pub struct Motion {
    velocity: Vec2,
    inverse_mass: f32,
    /// Tick-local marker, consumed by settle; never survives a completed step.
    driven: bool,
    /// Tick-local motor velocity change, drag included, consumed by the
    /// contact solver. It is spread across substeps like a force, so blocked
    /// motors lean on their contacts steadily instead of striking them once
    /// per tick, and a steady motor travels exactly at its requested speed.
    drive: Vec2,
}

impl Motion {
    /// Physical velocity in world X/Z, including actor locomotion.
    #[must_use]
    pub fn velocity(self) -> Vec2 {
        self.velocity
    }

    /// Reciprocal mass; a value of zero denotes an immovable physical body.
    #[must_use]
    pub fn inverse_mass(self) -> f32 {
        self.inverse_mass
    }
}

/// Sparse membership and its payload have one owner and one grant/revoke door.
#[derive(Default)]
pub(crate) struct Physics {
    who: Members,
    rows: Vec<Motion>,
    pub(crate) joints: crate::joint::Joints,
    pub(crate) solver: crate::solver::Solver,
}

/// A locomotion producer may accelerate existing movable bodies, never teleport,
/// replace velocity, grant physics or change mass.
pub(crate) struct DriveSink<'a> {
    physics: &'a mut Physics,
}

impl DriveSink<'_> {
    pub(crate) fn toward(&mut self, id: EntityId, desired: Vec2, dt: Dt, acceleration: f32) {
        assert!(desired.is_finite() && desired.length_squared().is_finite());
        let Some(row) = self.physics.who.index(id) else {
            return;
        };
        // Jointed bodies only move under impulses and contacts.
        if self.physics.articulated(id) {
            return;
        }
        let motion = &mut self.physics.rows[row];
        if motion.inverse_mass == 0.0 {
            return;
        }
        assert!(
            !motion.driven,
            "a body may receive only one motor request per tick"
        );
        let retained = motion.velocity * RETAIN;
        let target = retained + (desired - retained).clamp_length_max(acceleration * dt.secs());
        motion.drive = target - motion.velocity;
        motion.driven = true;
    }
}

/// Conservative discrete braking speed: leave enough distance to stop after
/// this tick's movement. `approach` is the speed toward the target before this
/// tick. A motor change ramps in over substeps, so braking still carries 3/8 of
/// that speed through each tick; reserve that distance up front.
/// Rationalized form avoids cancellation near arrival.
pub fn stopping_speed(distance: f32, approach: f32, dt: Dt, acceleration: f32) -> f32 {
    assert!(
        distance.is_finite()
            && distance >= 0.0
            && approach.is_finite()
            && acceleration.is_finite()
            && acceleration >= 0.0
    );
    let distance = distance - 0.375 * approach.max(0.0) * dt.secs();
    if distance <= 0.0 || acceleration == 0.0 {
        return 0.0;
    }
    let step = acceleration * dt.secs();
    let twice_work = 2.0 * acceleration * distance;
    twice_work / ((step * step + twice_work).sqrt() + step)
}

/// Solves v = RETAIN*v + acceleration*dt for the motor's steady speed.
pub fn motor_speed_limit(acceleration: f32) -> f32 {
    acceleration * Dt::SECS / (1.0 - RETAIN)
}

impl Physics {
    /// Bodies whose collision is their joint's box rather than a disc.
    pub(crate) fn articulated(&self, id: EntityId) -> bool {
        self.joints.index(id).is_some()
    }
    pub(crate) fn set_velocity(&mut self, id: EntityId, velocity: Vec2) {
        if self.articulated(id) {
            return;
        }
        if let Some(row) = self.who.index(id) {
            self.rows[row].velocity = velocity;
        }
    }
    pub(crate) fn drive(&mut self) -> DriveSink<'_> {
        DriveSink { physics: self }
    }
    /// Motor velocity change for this tick.
    pub(crate) fn take_drive(&mut self, id: EntityId) -> Vec2 {
        self.who
            .index(id)
            .map_or(Vec2::ZERO, |row| core::mem::take(&mut self.rows[row].drive))
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }
    pub(crate) fn grant(&mut self, id: EntityId, inverse_mass: f32) {
        assert!(inverse_mass.is_finite() && inverse_mass >= 0.0);
        if let Some(row) = self.who.add(id) {
            assert_eq!(row, self.rows.len());
            self.rows.push(Motion {
                velocity: Vec2::ZERO,
                inverse_mass,
                driven: false,
                drive: Vec2::ZERO,
            });
            self.solver.reserve(self.rows.len());
        }
    }

    /// Tuning changes mass without changing the body's current velocity.
    pub(crate) fn set_mass(&mut self, id: EntityId, mass: Mass) {
        if let Some(row) = self.who.index(id) {
            let inverse_mass = mass.kilograms().recip();
            if self.rows[row].inverse_mass != inverse_mass {
                self.rows[row].inverse_mass = inverse_mass;
                self.solver.clear_cache();
            }
        }
    }

    pub(crate) fn revoke(&mut self, id: EntityId) {
        self.joints.revoke(id);
        if let Some(row) = self.who.remove(id) {
            self.rows.swap_remove(row);
            self.solver.revoke(id);
        }
    }

    pub(crate) fn get(&self, id: EntityId) -> Option<Motion> {
        self.who.index(id).map(|row| {
            let mut motion = self.rows[row];
            if let Some(joint) = self.joints.get(id) {
                motion.velocity = joint.velocity_at(joint.collider().centre());
            }
            motion
        })
    }

    /// Shared physical hit geometry; allegiance and health are absent by design.
    pub(crate) fn disc_contact(
        &self,
        id: EntityId,
        position: Vec2,
        body_radius: f32,
        centre: Vec2,
        radius: f32,
    ) -> Option<Vec2> {
        if let Some(shape) = self.shape(id) {
            return shape
                .penetration(centre, radius)
                .map(|(normal, depth)| centre + normal * (depth - radius));
        }
        let reach = radius + body_radius;
        if centre.distance_squared(position) > reach * reach {
            return None;
        }
        Some(position + (centre - position).normalize_or(Vec2::X) * body_radius)
    }
    pub(crate) fn beam_contact(
        &self,
        id: EntityId,
        position: Vec2,
        body_radius: f32,
        beam: crate::Beam,
    ) -> Option<Vec2> {
        let axis = beam.end() - beam.start();
        if axis.length_squared() == 0.0 {
            return None;
        }
        if let Some(shape) = self.shape(id) {
            if let Some((normal, depth)) = shape.penetration(beam.start(), beam.radius()) {
                return Some(beam.start() + normal * (depth - beam.radius()));
            }
            return shape
                .sweep(beam.start(), axis, beam.radius())
                .map(|hit| hit.point() - hit.normal() * beam.radius());
        }
        beam.contact(position, body_radius)
    }
    /// Wall and jointed-body cover between two points; boxes are evaluated at
    /// their current pose, excluding the struck body.
    pub(crate) fn obstructs(
        &self,
        walls: &crate::wall::Walls,
        start: Vec2,
        end: Vec2,
        except: Option<EntityId>,
    ) -> bool {
        walls.obstructs(start, end)
            || self
                .joints
                .rows
                .iter()
                .any(|joint| Some(joint.id()) != except && joint.collider().obstructs(start, end))
    }
    pub(crate) fn solid_hit(
        &self,
        walls: &crate::wall::Walls,
        start: Vec2,
        end: Vec2,
    ) -> Option<crate::WallHit> {
        let mut nearest = walls.first_hit(start, end);
        for joint in &self.joints.rows {
            if let Some(hit) = joint.collider().ray_hit(start, end - start)
                && nearest.is_none_or(|current| hit.fraction() < current.fraction())
            {
                nearest = Some(hit);
            }
        }
        nearest
    }
    pub(crate) fn clip_beam(&self, beam: crate::Beam, walls: &crate::wall::Walls) -> crate::Beam {
        beam.clipped(self.solid_hit(walls, beam.start(), beam.end()))
    }
    pub(crate) fn joint(&self, id: EntityId) -> Option<crate::JointState> {
        self.joints.get(id)
    }
    /// The current box of a jointed body; discs have none.
    pub(crate) fn shape(&self, id: EntityId) -> Option<crate::Wall> {
        self.joint(id).map(crate::JointState::collider)
    }
    pub(crate) fn impulse_at(
        &mut self,
        id: EntityId,
        point: Vec2,
        impulse: Impulse,
        source: Option<EntityId>,
        trace: &mut TraceSink<'_>,
    ) -> bool {
        if !crate::valid_position(point) {
            return false;
        }
        if let Some(i) = self.joints.index(id) {
            let joint = &mut self.joints.rows[i];
            let at_arm = joint.at_arm(point, impulse.0);
            let moves_now = joint.lock != crate::LockState::Held && joint.config().planar();
            if moves_now {
                let rate = joint.rate + joint.rate_from(at_arm);
                if !admitted_point_speed_squared(joint.point_speed(rate).powi(2))
                    || !(rate / joint.inverse_inertia).is_finite()
                {
                    return false;
                }
                joint.rate = rate;
            } else {
                // The next contact solve applies it: a held lock absorbs it,
                // and an unlocked pole's base passes it on or its pivot takes it.
                let pending = joint.pending + at_arm;
                let rate = joint.rate + joint.rate_from(pending);
                if !pending.length_squared().is_finite()
                    || !admitted_point_speed_squared(joint.point_speed(rate).powi(2))
                {
                    return false;
                }
                joint.pending = pending;
            }
            trace.emit(Event::Impulsed {
                id,
                source,
                impulse,
            });
            return true;
        }
        self.impulse(id, impulse, source, trace)
    }

    pub(crate) fn impulse(
        &mut self,
        id: EntityId,
        impulse: Impulse,
        source: Option<EntityId>,
        trace: &mut TraceSink<'_>,
    ) -> bool {
        if let Some(joint) = self.joint(id) {
            return self.impulse_at(id, joint.collider().centre(), impulse, source, trace);
        }
        let Some(row) = self.who.index(id) else {
            return false;
        };
        let motion = &mut self.rows[row];
        let velocity = motion.velocity + impulse.0 * motion.inverse_mass;
        if !admitted_point_speed_squared(velocity.length_squared()) {
            return false;
        }
        motion.velocity = velocity;
        trace.emit(Event::Impulsed {
            id,
            source,
            impulse,
        });
        true
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self {
            who,
            rows,
            joints,
            solver,
        } = self;
        joints.hash(h);
        solver.hash(h);
        who.hash(h);
        h.usize(rows.len());
        for Motion {
            velocity,
            inverse_mass,
            driven,
            drive,
        } in rows
        {
            debug_assert!(!driven, "motor scratch must be consumed before hashing");
            debug_assert_eq!(*drive, Vec2::ZERO, "the solver consumes motor drive");
            h.f32(velocity.x);
            h.f32(velocity.y);
            h.f32(*inverse_mass);
        }
    }

    pub(crate) fn finite(&self) -> bool {
        self.rows
            .iter()
            .all(|m| m.velocity.is_finite() && m.inverse_mass.is_finite())
            && self.joints.finite()
    }

    pub(crate) fn report(&self, pos: &[Vec2], slots: &Slots, out: &mut Report) {
        let Self {
            who,
            rows,
            joints,
            solver,
        } = self;
        joints.report(out);
        solver.report(out);
        out.object("bodies", |out| {
            for (
                &id,
                &Motion {
                    velocity,
                    inverse_mass,
                    driven: _,
                    drive: _,
                },
            ) in who.ids().iter().zip(rows)
            {
                let Some(row) = slots.index(id) else { continue };
                out.object(&id.to_string(), |out| {
                    out.vec3("pos", crate::on_ground(pos[row], 0.0));
                    let velocity = joints
                        .get(id)
                        .map_or(velocity, |j| j.velocity_at(j.collider().centre()));
                    out.vec3("velocity", crate::on_ground(velocity, 0.0));
                    out.num("inverse_mass", inverse_mass);
                });
            }
        });
    }
}

/// After contacts and walls, damp bodies whose drag was not consumed by a motor.
/// Before attacks: a new impulse changes velocity now and moves next tick.
/// A motor may damp and oppose it before that next integration.
pub(crate) fn settle(physics: &mut Physics, _dt: Dt, mut trace: TraceSink<'_>) {
    for joint in &mut physics.joints.rows {
        if joint.config().planar() {
            joint.rate *= RETAIN;
        }
    }
    let mut stopped = 0;
    for motion in &mut physics.rows {
        if core::mem::take(&mut motion.driven) {
            continue;
        }
        if motion.velocity == Vec2::ZERO {
            continue;
        }
        motion.velocity *= RETAIN;
        if motion.velocity.length_squared() < REST_SPEED * REST_SPEED {
            motion.velocity = Vec2::ZERO;
            stopped += 1;
        }
    }
    if stopped > 0 {
        trace.emit(Event::Rested { count: stopped });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motor_boundary_canonicalizes_only_normalization_roundoff() {
        let actor = crate::World::empty().player_id();
        for direction in [Vec2::ONE, Vec2::new(0.1, 0.7), Vec2::new(3.0, 5.0), Vec2::X] {
            let velocity = direction.normalize() * MAX_MOTOR_SPEED;
            let command = MotorCommand::new(actor, velocity, MAX_MOTOR_ACCELERATION).unwrap();
            assert!(command.velocity.length_squared() <= MAX_MOTOR_SPEED * MAX_MOTOR_SPEED);
            assert!((command.velocity.length() - MAX_MOTOR_SPEED).abs() < 0.00002);
        }
        assert!(MotorCommand::new(actor, Vec2::X * 30.001, 30.0).is_err());
        assert!(MotorCommand::new(actor, Vec2::X, 300.001).is_err());
    }

    #[test]
    fn nonfinite_impulses_cannot_enter_the_simulation() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            assert!(Impulse::try_from((value, 0.0)).is_err());
            assert!(Impulse::try_from((0.0, value)).is_err());
        }
    }
}
