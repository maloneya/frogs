//! Unlocked poles: rigid rods turning about their base under gravity.
//!
//! Runs after the contact solve, so the load a pole's base absorbed this tick,
//! and bodies' final positions, are known. A pole passes over bodies and meets
//! each as it comes down through its contact height there; that meeting
//! exchanges momentum along the fall, inelastically, between the body and
//! the pole's rotation. Bodies give no vertical support, so the pole continues
//! to the ground and lies flat.
//!
//! Walls, other jointed bodies and standing poles are treated as at least as
//! tall as any pole. A fall that would cross one stops leaning against it; the
//! angular momentum it loses is delivered there at the contact height, so a
//! leaning pole keeps loading a lock while gravity pulls it. A pole released
//! without load stands balanced until a load picks its direction.
//! Met bodies move from the next physics step, like attack impulses.

use core::f32::consts::FRAC_PI_2;
use glam::Vec2;

use super::motion::{Impulse, Physics};
use crate::joint::{Impact, JointState, Kind, LockState, ToppleConfig, gravity};
use crate::trace::{Event, TraceSink};
use crate::{Dt, EntityId, PLAYER_RADIUS, Wall, body_radius};

const SUBSTEPS: usize = 4;
/// Halvings when locating where a fall first meets a solid.
const BISECTIONS: usize = 12;

/// Something tall in the way of a fall, and where to load it.
enum Blocker {
    Solid,
    Body(EntityId, Vec2),
}

pub(crate) fn topple(
    physics: &mut Physics,
    ids: &[EntityId],
    positions: &[Vec2],
    walls: &crate::wall::Walls,
    grid: &mut crate::grid::Grid,
    dt: Dt,
    mut trace: TraceSink<'_>,
) {
    physics.joints.impacts.clear();
    if !physics.joints.rows.iter().any(|j| toppling(*j)) {
        return;
    }
    // The solver's grid holds an earlier pass's positions; impacts read final ones.
    grid.build(positions, |row| !physics.articulated(ids[row]));
    let h = dt.secs() / SUBSTEPS as f32;
    let mut candidates = core::mem::take(&mut physics.joints.candidates);
    let mut met = Vec::new();
    for index in 0..physics.joints.rows.len() {
        let Some(mut fall) = Fall::start(index, physics.joints.rows[index]) else { continue };
        let anchor = fall.joint.anchor;
        let tip = anchor + fall.direction() * fall.pole.height();
        let widen = Vec2::splat(fall.pole.sweep() + PLAYER_RADIUS);
        candidates.clear();
        grid.query(anchor.min(tip) - widen, anchor.max(tip) + widen, |row| {
            candidates.push(row as u32);
        });
        candidates.sort_unstable();
        core::mem::swap(&mut met, &mut physics.joints.met[index]);
        for _ in 0..SUBSTEPS {
            let t = &mut fall.joint;
            t.rate += gravity(fall.pole, t.angle) * h;
            let mut next = (t.angle + t.rate * h).clamp(0.0, FRAC_PI_2);
            if next == 0.0 {
                t.rate = t.rate.max(0.0);
            }
            if next > fall.joint.angle
                && let Some(blocker) = fall.blocked(physics, walls, next)
            {
                let (free, blocker) = fall.settle_against(physics, walls, next, blocker);
                next = free;
                fall.stop(physics, blocker, &mut trace);
            }
            fall.joint.angle = next;
            fall.strike(physics, (ids, positions), &candidates, &mut met, &mut trace);
            if fall.joint.angle == FRAC_PI_2 {
                fall.joint.rate = 0.0;
                trace.emit(Event::Toppled { id: fall.joint.id });
                break;
            }
        }
        core::mem::swap(&mut met, &mut physics.joints.met[index]);
        physics.joints.rows[index] = fall.joint;
    }
    physics.joints.candidates = candidates;
}

/// An unlocked pole still above the ground.
fn toppling(j: JointState) -> bool {
    matches!(j.kind, Kind::Topple { .. }) && j.lock != LockState::Held && !j.grounded()
}

/// One pole coming down during this pass, at `index` in the joint rows.
struct Fall {
    index: usize,
    joint: JointState,
    pole: ToppleConfig,
}
impl Fall {
    /// A toppling pole with its tick's base load applied, once a load has
    /// picked the direction it falls in.
    fn start(index: usize, mut joint: JointState) -> Option<Self> {
        let Kind::Topple { config: pole, direction, load } = joint.kind else { return None };
        if !toppling(joint) {
            return None;
        }
        let direction = if direction != Vec2::ZERO {
            direction
        } else if load != Vec2::ZERO {
            load.normalize()
        } else {
            return None;
        };
        joint.kind = Kind::Topple { config: pole, direction, load };
        joint.rate += joint.rate_from(load);
        if joint.angle == 0.0 {
            // Hinged on its base edge: it cannot tip backwards past upright.
            joint.rate = joint.rate.max(0.0);
        }
        Some(Self { index, joint, pole })
    }
    fn direction(&self) -> Vec2 {
        self.joint.direction()
    }

    /// The first tall solid the span beyond the base would overlap at `tilt`.
    fn blocked(&self, physics: &Physics, walls: &crate::wall::Walls, tilt: f32) -> Option<Blocker> {
        let (anchor, direction) = (self.joint.anchor, self.direction());
        // A pole that would lie partly outside the arena stops at its edge.
        if tilt == FRAC_PI_2 && self.pole.lying(anchor, direction).is_err() {
            return Some(Blocker::Solid);
        }
        let Ok(span) = self.pole.span(anchor, direction, tilt) else {
            return Some(Blocker::Solid);
        };
        let span = span?;
        // Overlap must exceed the skin, so a pole resting against a face is free.
        let touches = |shape: Wall| {
            if !shape.bounds_within(span, 0.0) {
                return None;
            }
            shape.rectangle_contact(span, -crate::wall::SKIN).map(|(_, points, _)| points[0].unwrap_or(shape.centre()))
        };
        if walls.iter().any(|(_, _, wall)| touches(wall).is_some()) {
            return Some(Blocker::Solid);
        }
        for (j, other) in physics.joints.rows.iter().enumerate() {
            // Poles lying flat are low; everything else jointed is tall.
            if j != self.index && !other.grounded() && let Some(point) = touches(other.collider()) {
                return Some(Blocker::Body(other.id(), point));
            }
        }
        None
    }

    /// Largest tilt short of `blocked_at` that is free, and what blocks beyond it.
    fn settle_against(&self, physics: &Physics, walls: &crate::wall::Walls, blocked_at: f32, mut blocker: Blocker) -> (f32, Blocker) {
        let (mut free, mut hit) = (self.joint.angle, blocked_at);
        if let Some(already) = self.blocked(physics, walls, free) {
            // Something moved into the pole's span; it holds where it is.
            return (free, already);
        }
        for _ in 0..BISECTIONS {
            let middle = 0.5 * (free + hit);
            match self.blocked(physics, walls, middle) {
                Some(found) => {
                    hit = middle;
                    blocker = found;
                }
                None => free = middle,
            }
        }
        (free, blocker)
    }

    /// Stopping the rotation takes its angular momentum; a jointed blocker
    /// receives it as an impulse along the fall at the contact height.
    fn stop(&mut self, physics: &mut Physics, blocker: Blocker, trace: &mut TraceSink<'_>) {
        let momentum = self.joint.momentum(self.joint.rate);
        self.joint.rate = 0.0;
        let Blocker::Body(id, point) = blocker else { return };
        let push = self.direction() * momentum;
        if momentum > 0.0
            && let Ok(impulse) = Impulse::try_from((push.x, push.y))
        {
            physics.impulse_at(id, point, impulse, Some(self.joint.id), trace);
        }
    }

    /// Meet every candidate the pole has come down on at its current tilt.
    /// Candidates are rows of the immutable id and position view.
    fn strike(
        &mut self,
        physics: &mut Physics,
        (ids, positions): (&[EntityId], &[Vec2]),
        candidates: &[u32],
        met: &mut Vec<EntityId>,
        trace: &mut TraceSink<'_>,
    ) {
        let t = &mut self.joint;
        let direction = t.direction();
        let (sin, cos) = t.angle.sin_cos();
        let height = self.pole.contact_height();
        for &row in candidates {
            let row = row as usize;
            let id = ids[row];
            let offset = positions[row] - t.anchor;
            let along = offset.dot(direction);
            let radius = body_radius(row);
            if along < 0.0 || (offset - direction * along).length() > self.pole.sweep() + radius {
                continue;
            }
            // The pole is down to the contact height over the body's nearest edge.
            let near = (along - radius).max(0.0);
            if near > self.pole.height() * sin || near * cos > height * sin || met.contains(&id) {
                continue;
            }
            let motion = physics.get(id).expect("candidate is a live body");
            // At height y the pole moves horizontally at rate * y, wherever along it.
            let closing = t.rate * height - motion.velocity().dot(direction);
            let lever = height * height * t.inverse_inertia;
            let mut delivered = 0.0;
            if motion.inverse_mass() > 0.0 && closing > 0.0 {
                let amount = closing / (motion.inverse_mass() + lever);
                let push = direction * amount;
                if let Ok(impulse) = Impulse::try_from((push.x, push.y))
                    && physics.impulse(id, impulse, Some(t.id), trace)
                {
                    t.rate -= t.rate_from(push);
                    delivered = amount;
                }
            }
            met.push(id);
            physics.joints.impacts.push(Impact {
                source: t.id,
                body: id,
                point: positions[row],
                closing_speed: closing,
                impulse: delivered,
                damaged: false,
            });
        }
    }
}
