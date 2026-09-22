//! Resolve powered movement, impulses and later crowd corrections against walls.
use super::motion::Physics;
use crate::{ENEMY_RADIUS, EntityId, Event, PLAYER_RADIUS, TraceSink, wall::Walls};
use glam::Vec2;

pub(crate) fn resolve(
    walls: &Walls,
    previous: &[Vec2],
    positions: &mut [Vec2],
    ids: &[EntityId],
    physics: &mut Physics,
    mut trace: TraceSink<'_>,
) {
    if walls.is_empty() {
        return;
    }
    let mut count = 0;
    for (row, ((position, &start), &id)) in positions.iter_mut().zip(previous).zip(ids).enumerate()
    {
        if physics
            .get(id)
            .is_none_or(|motion| motion.inverse_mass() == 0.0)
        {
            continue;
        }
        let radius = if row == 0 {
            PLAYER_RADIUS
        } else {
            ENEMY_RADIUS
        };
        let mut touched = false;
        *position = walls.slide(start, *position, radius, |normal| {
            // Arena normals point into the solid; rectangle normals point out.
            physics.wall(id, -normal);
            touched = true;
        });
        count += usize::from(touched);
    }
    if count > 0 {
        trace.emit(Event::WallContacts { count });
    }
}
