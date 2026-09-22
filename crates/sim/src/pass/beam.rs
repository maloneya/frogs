//! Held channels suppress powered walking, but never physical displacement.
//! Damage reads final positions after contact and lands before defeat removal.
use super::{attack::Attack, health::DamageSink};
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

pub(crate) fn strike(
    beam: Beam,
    ids: &[EntityId],
    positions: &[Vec2],
    walls: &crate::wall::Walls,
    damage: &mut DamageSink<'_>,
    trace: &mut TraceSink<'_>,
) {
    for (&id, &position) in ids.iter().zip(positions) {
        if beam.touches(position, ENEMY_RADIUS)
            && !walls.obstructs(beam.start(), position)
            && let Some(remaining) = damage.hit(id)
        {
            trace.emit(Event::Hit { id, remaining });
        }
    }
}
