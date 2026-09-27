//! Held channels suppress powered walking, but never physical displacement.
//! Damage reads final positions after contact and lands before defeat removal.
use super::{attack::Attack, health::DamageSink, motion::{Impulse, ImpulseSink}};
use crate::{
    Beam, ENEMY_RADIUS, EntityId,
    trace::{Event, TraceSink},
};
use arpg_core::{Intent, MoveDir};
use glam::Vec2;

pub(crate) fn movement(attack: &Attack, intent: Intent) -> MoveDir {
    if attack.channels(intent.attack_held()) {
        MoveDir::NONE
    } else {
        intent.move_dir()
    }
}

/// Every touching tick adds momentum; only pulse ticks damage. Both read final
/// positions, so the new velocity first displaces a body on the following tick.
#[expect(clippy::too_many_arguments, reason = "separate impulse and damage capabilities keep the pass out of mutable World")]
pub(crate) fn strike(
    actor: EntityId,
    beam: Beam,
    pulse: bool,
    ids: &[EntityId],
    positions: &[Vec2],
    walls: &crate::wall::Walls,
    damage: &mut DamageSink<'_>,
    impulses: &mut ImpulseSink<'_>,
    hits: &mut Vec<Vec2>,
    trace: &mut TraceSink<'_>,
) {
    let push = (beam.end() - beam.start()).normalize_or_zero() * Beam::KNOCKBACK;
    let impulse = Impulse::try_from((push.x, push.y)).expect("finite beam tuning");
    for (&id, &position) in ids.iter().zip(positions) {
        if id == actor || !beam.touches(position, ENEMY_RADIUS)
            || walls.obstructs(beam.start(), position)
            || !damage.contains(id)
        { continue; }
        impulses.push(id, impulse, trace);
        if pulse && let Some(remaining) = damage.hit(id, core::num::NonZeroU8::MIN) {
            hits.push(position);
            trace.emit(Event::Hit { id, remaining });
            trace.emit(Event::Damaged { actor, target: id, remaining });
        }
    }
}
