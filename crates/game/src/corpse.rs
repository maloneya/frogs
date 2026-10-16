//! Bounded visual remains, admitted from completed combat outcomes only.
use arpg_core::Report;
use arpg_sim::{ActorView, Alpha, Defeat, DefeatedBodyRetirement, EntityId, Fnv, SceneId};
use glam::Vec2;

/// Maximum simultaneous remains. Full admission skips subsequent deaths.
pub const MAX_CORPSES: usize = 128;
/// Ten seconds measured in fixed simulation ticks.
pub const CORPSE_LIFETIME_TICKS: u32 = 10 * arpg_sim::TICK_HZ;

/// Read-only remains from the lethal tick; shared physics may still carry the body.
#[derive(Clone, Copy, Debug)]
pub struct Corpse {
    body: EntityId,
    scene: Option<SceneId>,
    position: Vec2,
    previous: Vec2,
    remaining: u32,
}
impl Corpse {
    /// Defeated generational identity, retained after physics removal.
    pub fn id(self) -> EntityId { self.body }
    /// Latest ground position, following the physical body until it rests.
    pub fn position(self) -> Vec2 { self.position }
    /// Interpolated ground position through the latest physical step.
    pub fn drawn_position(self, alpha: Alpha) -> Vec2 { self.previous.lerp(self.position, alpha.get()) }
    /// Number of further fixed steps before removal.
    pub fn remaining_ticks(self) -> u32 { self.remaining }
}

pub(crate) struct Corpses { rows: Vec<Corpse> }
impl Default for Corpses {
    fn default() -> Self { Self { rows: Vec::with_capacity(MAX_CORPSES) } }
}
impl Corpses {
    pub(crate) fn as_slice(&self) -> &[Corpse] { &self.rows }
    pub(crate) fn remove(&mut self, id: EntityId) { self.rows.retain(|row| row.body != id); }
    pub(crate) fn evict(&mut self, scene: SceneId) { self.rows.retain(|row| row.scene != Some(scene)); }
    pub(crate) fn hash(&self, hash: &mut Fnv) {
        hash.usize(self.rows.len());
        for row in &self.rows {
            row.body.hash_into(hash);
            hash.usize(usize::from(row.scene.is_some()));
            if let Some(scene) = row.scene { scene.hash_into(hash); }
            hash.f32(row.previous.x);
            hash.f32(row.previous.y);
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
                out.num("previous_x", row.previous.x);
                out.num("previous_z", row.previous.y);
                out.num("x", row.position.x);
                out.num("z", row.position.y);
                out.int("remaining_ticks", u64::from(row.remaining));
            });
        });
    }
}

/// Observe final physical positions before the structural removal pass. Once the
/// body rests, the same record keeps its final position and death age.
pub(crate) fn follow(corpses: &mut Corpses, view: ActorView<'_>) {
    for row in &mut corpses.rows {
        row.previous = row.position;
        if let Some(position) = view.position(row.body) { row.position = position; }
    }
}

/// Expiration and admission share the lethal-tick clock. Overflow retires the
/// unrepresented body immediately; neither pressure nor a held beam can evade the cap.
pub(crate) fn advance<'a>(
    corpses: &mut Corpses,
    defeats: impl Iterator<Item = &'a Defeat>,
    retirement: &mut DefeatedBodyRetirement<'_>,
) {
    corpses.rows.retain_mut(|row| {
        row.remaining -= 1;
        if row.remaining == 0 { retirement.retire(row.body); }
        row.remaining > 0
    });
    for defeat in defeats {
        if corpses.rows.len() == MAX_CORPSES {
            retirement.retire(defeat.body);
            continue;
        }
        corpses.rows.push(Corpse {
            body: defeat.body, scene: defeat.scene, position: defeat.position,
            previous: retirement.previous_position(defeat.body).unwrap_or(defeat.position),
            remaining: CORPSE_LIFETIME_TICKS,
        });
    }
}
