//! Carried motion: impulses change velocity; contacts exchange momentum.
//!
//! Walk and seek remain powered, kinematic locomotion. Their displacement is
//! not accumulated into velocity. This store owns the additional motion that
//! survives a tick, so steering cannot overwrite a blow and overlap correction
//! cannot create kinetic energy. Every physical body has a membership and a
//! payload; neither attack nor the renderer owns any of this state.

use glam::Vec2;
use serde::Deserialize;

use crate::members::Members;
use crate::slots::Slots;
use crate::trace::{Event, TraceSink};
use crate::{Dt, EntityId, Fnv};
use arpg_core::Report;

/// The existing crowd mass ratio, now shared by projection and impulses.
pub(crate) const PLAYER_INV_MASS: f32 = 0.05;
pub(crate) const ENEMY_INV_MASS: f32 = 1.0;
const _: () = assert!(PLAYER_INV_MASS > 0.0 && PLAYER_INV_MASS < ENEMY_INV_MASS);

/// Retain 90% of carried speed per simulation tick (60 Hz), after collisions.
/// Geometric decay gives a free body a total travel of v / 6 world units.
const RETAIN: f32 = 0.9;
/// Snap the invisible tail to rest, so a settled body really has zero velocity.
const REST_SPEED: f32 = 0.001;
const _: () = assert!(RETAIN > 0.0 && RETAIN < 1.0 && REST_SPEED > 0.0);

/// A finite world-ground-plane momentum change, shared by gameplay and readers.
/// Deserializing uses the same validation as an engine caller.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(try_from = "(f32, f32)")]
pub struct Impulse(Vec2);

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
}

impl Motion {
    /// Carried velocity in world X/Z, excluding powered locomotion.
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
}

/// A producer can add momentum, but cannot integrate, grant, revoke or set mass.
pub(crate) struct ImpulseSink<'a> {
    physics: &'a mut Physics,
    source: EntityId,
}

impl ImpulseSink<'_> {
    pub(crate) fn push(&mut self, id: EntityId, impulse: Impulse, trace: &mut TraceSink<'_>) {
        self.physics.impulse(id, impulse, Some(self.source), trace);
    }
}

impl Physics {
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }
    pub(crate) fn sink(&mut self, source: EntityId) -> ImpulseSink<'_> {
        ImpulseSink { physics: self, source }
    }

    pub(crate) fn grant(&mut self, id: EntityId, inverse_mass: f32) {
        assert!(inverse_mass.is_finite() && inverse_mass >= 0.0);
        if let Some(row) = self.who.add(id) {
            assert_eq!(row, self.rows.len());
            self.rows.push(Motion { velocity: Vec2::ZERO, inverse_mass });
        }
    }

    pub(crate) fn revoke(&mut self, id: EntityId) {
        if let Some(row) = self.who.remove(id) {
            self.rows.swap_remove(row);
        }
    }

    pub(crate) fn get(&self, id: EntityId) -> Option<Motion> {
        self.who.index(id).map(|row| self.rows[row])
    }

    pub(crate) fn impulse(
        &mut self,
        id: EntityId,
        impulse: Impulse,
        source: Option<EntityId>,
        trace: &mut TraceSink<'_>,
    ) -> bool {
        let Some(row) = self.who.index(id) else { return false };
        let motion = &mut self.rows[row];
        let velocity = motion.velocity + impulse.0 * motion.inverse_mass;
        assert!(
            velocity.is_finite() && velocity.length_squared().is_finite(),
            "impulses overflowed velocity"
        );
        motion.velocity = velocity;
        trace.emit(Event::Impulsed { id, source, impulse });
        true
    }

    /// Perfectly inelastic response along the normal. Only closing velocity
    /// generates an impulse; tangential velocity is unchanged. Equal and
    /// opposite impulses conserve the pair's momentum, including unequal mass.
    pub(crate) fn collide(
        &mut self,
        a: EntityId,
        b: EntityId,
        contact: crate::contact::Contact,
    ) -> bool {
        let normal = contact.normal();
        let (Some(i), Some(j)) = (self.who.index(a), self.who.index(b)) else { return false };
        let (a, b) = (self.rows[i], self.rows[j]);
        let closing = (b.velocity - a.velocity).dot(normal);
        let mass = a.inverse_mass + b.inverse_mass;
        if closing >= 0.0 || mass == 0.0 {
            return false;
        }
        let impulse = normal * (-closing / mass);
        self.rows[i].velocity -= impulse * a.inverse_mass;
        self.rows[j].velocity += impulse * b.inverse_mass;
        true
    }

    /// A wall removes only the component moving outward, preserving sliding.
    pub(crate) fn wall(&mut self, id: EntityId, normal: Vec2) {
        let Some(row) = self.who.index(id) else { return };
        let motion = &mut self.rows[row];
        let outward = motion.velocity.dot(normal);
        if outward > 0.0 {
            motion.velocity -= normal * outward;
        }
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { who, rows } = self;
        who.hash(h);
        h.usize(rows.len());
        for Motion { velocity, inverse_mass } in rows {
            h.f32(velocity.x);
            h.f32(velocity.y);
            h.f32(*inverse_mass);
        }
    }

    pub(crate) fn finite(&self) -> bool {
        self.rows.iter().all(|m| m.velocity.is_finite() && m.inverse_mass.is_finite())
    }

    pub(crate) fn report(&self, pos: &[Vec2], slots: &Slots, out: &mut Report) {
        let Self { who, rows } = self;
        out.object("bodies", |out| {
            for (&id, &Motion { velocity, inverse_mass }) in who.ids().iter().zip(rows) {
                let Some(row) = slots.index(id) else { continue };
                out.object(&id.to_string(), |out| {
                    out.vec3("pos", crate::on_ground(pos[row], 0.0));
                    out.vec3("velocity", crate::on_ground(velocity, 0.0));
                    out.num("inverse_mass", inverse_mass);
                });
            }
        });
    }
}

/// After powered locomotion, before contacts: integrate carried velocity once.
/// Resolving contacts sees the resulting positions, including a knocked body
/// entering its neighbour. No correction is ever fed back as velocity.
pub(crate) fn integrate(physics: &Physics, slots: &Slots, pos: &mut [Vec2], dt: Dt) {
    for (&id, motion) in physics.who.ids().iter().zip(&physics.rows) {
        if let Some(row) = slots.index(id) {
            pos[row] += motion.velocity * dt.secs();
        }
    }
}

/// After contacts and walls, before attacks. A new attack impulse retains its
/// full magnitude until the next tick's integration, so hit latency is explicit.
pub(crate) fn settle(physics: &mut Physics, _dt: Dt, mut trace: TraceSink<'_>) {
    let mut stopped = 0;
    for motion in &mut physics.rows {
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

    fn contact(normal: Vec2) -> crate::contact::Contact {
        crate::contact::between(Vec2::ZERO, normal * 0.5, 1.0, 0).unwrap()
    }

    #[test]
    fn a_contact_conserves_momentum_and_cannot_add_energy() {
        let mut slots = Slots::default();
        let (a, b) = (slots.insert(), slots.insert());
        let mut physics = Physics::default();
        physics.grant(a, 0.5); // mass 2
        physics.grant(b, 1.0); // mass 1
        let mut trace = crate::Trace::default();
        physics.impulse(a, Impulse::try_from((12.0, 4.0)).unwrap(), None, &mut trace.sink(0));
        physics.impulse(b, Impulse::try_from((-3.0, -1.0)).unwrap(), None, &mut trace.sink(0));
        let totals = |p: &Physics| {
            let (a, b) = (p.get(a).unwrap().velocity(), p.get(b).unwrap().velocity());
            (2.0 * a + b, a.length_squared() + 0.5 * b.length_squared())
        };
        let (momentum, energy) = totals(&physics);
        assert!(physics.collide(a, b, contact(Vec2::X)));
        let (after_momentum, after_energy) = totals(&physics);
        assert!((after_momentum - momentum).length() < 1e-6);
        assert!(after_energy <= energy);
        assert_eq!(physics.get(a).unwrap().velocity(), Vec2::new(3.0, 2.0));
        assert_eq!(physics.get(b).unwrap().velocity(), Vec2::new(3.0, -1.0));
        assert!(
            !physics.collide(a, b, contact(Vec2::X)),
            "equal normal velocity needs no further impulse"
        );
        assert!(
            !physics.collide(a, b, contact(Vec2::NEG_Y)),
            "separating bodies must not be pulled together"
        );
    }

    #[test]
    fn an_immovable_body_absorbs_without_acquiring_velocity() {
        let mut slots = Slots::default();
        let (wall, body) = (slots.insert(), slots.insert());
        let mut physics = Physics::default();
        physics.grant(wall, 0.0);
        physics.grant(body, 1.0);
        let mut trace = crate::Trace::default();
        physics.impulse(body, Impulse::try_from((-6.0, 3.0)).unwrap(), None, &mut trace.sink(0));
        assert!(physics.collide(wall, body, contact(Vec2::X)));
        assert_eq!(physics.get(wall).unwrap().velocity(), Vec2::ZERO);
        assert_eq!(physics.get(body).unwrap().velocity(), Vec2::new(0.0, 3.0));
    }

    #[test]
    fn nonfinite_impulses_cannot_enter_the_simulation() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            assert!(Impulse::try_from((value, 0.0)).is_err());
            assert!(Impulse::try_from((0.0, value)).is_err());
        }
    }
}
