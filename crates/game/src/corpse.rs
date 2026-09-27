//! Bounded visual remains, admitted from completed combat outcomes only.
use arpg_core::Report;
use arpg_sim::{Defeat, EntityId, Fnv, SceneId};
use glam::Vec2;

/// Maximum simultaneous remains. Full admission skips subsequent deaths.
pub const MAX_CORPSES: usize = 128;
/// Ten seconds measured in fixed simulation ticks.
pub const CORPSE_LIFETIME_TICKS: u32 = 10 * arpg_sim::TICK_HZ;

/// Read-only nonphysical remains; never registered as a body or attack target.
#[derive(Clone, Copy, Debug)]
pub struct Corpse {
    body: EntityId,
    scene: Option<SceneId>,
    position: Vec2,
    remaining: u32,
}
impl Corpse {
    /// Retired identity, used only for stable visual variation.
    pub fn id(self) -> EntityId { self.body }
    /// Ground position at defeat.
    pub fn position(self) -> Vec2 { self.position }
    /// Number of further fixed steps before removal.
    pub fn remaining_ticks(self) -> u32 { self.remaining }
}

pub(crate) struct Corpses { rows: Vec<Corpse> }
impl Default for Corpses {
    fn default() -> Self { Self { rows: Vec::with_capacity(MAX_CORPSES) } }
}
impl Corpses {
    pub(crate) fn as_slice(&self) -> &[Corpse] { &self.rows }
    pub(crate) fn evict(&mut self, scene: SceneId) { self.rows.retain(|row| row.scene != Some(scene)); }
    pub(crate) fn hash(&self, hash: &mut Fnv) {
        hash.usize(self.rows.len());
        for row in &self.rows {
            row.body.hash_into(hash);
            hash.usize(usize::from(row.scene.is_some()));
            if let Some(scene) = row.scene { scene.hash_into(hash); }
            hash.f32(row.position.x);
            hash.f32(row.position.y);
            hash.u64(u64::from(row.remaining));
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.int("corpse_count", self.rows.len() as u64);
        out.object("corpses", |out| for row in &self.rows {
            out.object(&row.body.to_string(), |out| {
                if let Some(scene) = row.scene { out.text("scene", &scene.to_string()); }
                out.num("x", row.position.x);
                out.num("z", row.position.y);
                out.int("remaining_ticks", u64::from(row.remaining));
            });
        });
    }
}

/// After physics removal: expire old remains first, then admit this tick's
/// deaths in engine order. No diagnostic history or mutable physics access.
pub(crate) fn advance<'a>(corpses: &mut Corpses, defeats: impl Iterator<Item = &'a Defeat>) {
    corpses.rows.retain_mut(|row| {
        row.remaining -= 1;
        row.remaining > 0
    });
    for defeat in defeats.take(MAX_CORPSES - corpses.rows.len()) {
        corpses.rows.push(Corpse {
            body: defeat.body, scene: defeat.scene, position: defeat.position,
            remaining: CORPSE_LIFETIME_TICKS,
        });
    }
}
