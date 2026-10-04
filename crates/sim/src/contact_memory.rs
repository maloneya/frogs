//! Warm-start memory: the force each persistent contact carried last step.
//!
//! Starting a fixed solver budget from last step's answer lets steady crowd
//! pressure build across ticks instead of being recomputed from zero, which
//! is what lets a deep crowd load a latch without iterating to a tolerance.
//! Forces rather than impulses are stored so a changed substep length
//! rescales them correctly. This is physical state: it is hashed and reported.
use crate::{EntityId, Fnv, SceneId};
use arpg_core::Report;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// A contact's identity; `point` distinguishes the two ends of an edge contact.
pub(crate) enum Key {
    Bodies(EntityId, EntityId, u8),
    Wall(EntityId, SceneId, usize, u8),
    Arena(EntityId, u8),
}
impl Key {
    pub(crate) fn bodies(a: EntityId, b: EntityId) -> Self {
        Self::Bodies(a.min(b), a.max(b), 0)
    }
    pub(crate) fn point(self, point: u8) -> Self {
        match self {
            Self::Bodies(a, b, _) => Self::Bodies(a, b, point),
            Self::Wall(body, owner, index, _) => Self::Wall(body, owner, index, point),
            Self::Arena(..) => self,
        }
    }
    fn ids(self) -> (EntityId, Option<EntityId>) {
        match self {
            Self::Bodies(a, b, _) => (a, Some(b)),
            Self::Wall(body, ..) | Self::Arena(body, _) => (body, None),
        }
    }
    fn hash(self, h: &mut Fnv) {
        match self {
            Self::Bodies(a, b, point) => {
                h.u64(0);
                a.hash_into(h);
                b.hash_into(h);
                h.u64(u64::from(point));
            }
            Self::Wall(body, owner, index, point) => {
                h.u64(1);
                body.hash_into(h);
                owner.hash_into(h);
                h.usize(index);
                h.u64(u64::from(point));
            }
            Self::Arena(body, side) => {
                h.u64(2);
                body.hash_into(h);
                h.u64(u64::from(side));
            }
        }
    }
}
impl core::fmt::Display for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Bodies(a, b, 0) => write!(f, "{a}/{b}"),
            Self::Bodies(a, b, point) => write!(f, "{a}/{b}#{point}"),
            Self::Wall(body, owner, index, 0) => write!(f, "{body}/{owner}/wall{index}"),
            Self::Wall(body, owner, index, point) => {
                write!(f, "{body}/{owner}/wall{index}#{point}")
            }
            Self::Arena(body, side) => write!(f, "{body}/arena{side}"),
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Contact {
    pub(crate) key: Key,
    /// Canonical normal: from the lower body identity toward the higher, or
    /// from a solid toward its body.
    pub(crate) normal: Vec2,
    /// Normal force in newtons.
    pub(crate) force: f32,
}
#[derive(Default)]
pub(crate) struct Memory {
    accepted: Vec<Contact>,
    next: Vec<Contact>,
    // A derived open-addressed index accelerates exact-key queries without
    // changing the canonical order used for physical state, hashing or
    // reporting. Per-body lists made a coincident pile of k bodies cost k³.
    index: Vec<u32>,
    // Removals and evictions are recorded in O(1) and filtered on read; the
    // next accepted step rewrites the memory from live contacts only. This
    // keeps a mass death from rebuilding the whole memory once per body.
    revoked: Vec<Option<EntityId>>,
    revoked_slots: Vec<usize>,
    evicted: Vec<SceneId>,
}
const NONE: u32 = u32::MAX;

/// Lookup index position. Generations and scene identity are left to the
/// exact key comparison: slots already spread live contacts, and nothing
/// iterates the index, so its layout cannot reach physical state.
fn bucket(key: Key, mask: usize) -> usize {
    let (low, high, tag) = match key {
        Key::Bodies(a, b, point) => (a.slot(), b.slot(), u64::from(point)),
        Key::Wall(body, _, index, point) => (body.slot(), index, 4 | u64::from(point)),
        Key::Arena(body, side) => (body.slot(), 0, 8 | u64::from(side)),
    };
    let mixed = (low as u64 | (high as u64) << 28 | tag << 56).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    // The multiply leaves its best-mixed bits at the top.
    (mixed >> 32) as usize & mask
}
impl Memory {
    pub(crate) fn reserve(&mut self, bodies: usize, contacts: usize) {
        crate::reserve_capacity(&mut self.accepted, contacts);
        crate::reserve_capacity(&mut self.next, contacts);
        crate::reserve_capacity(&mut self.index, 2 * contacts.next_power_of_two());
        crate::reserve_capacity(&mut self.revoked, bodies);
        crate::reserve_capacity(&mut self.revoked_slots, bodies);
    }
    fn stale(&self, key: Key) -> bool {
        let gone = |id: EntityId| self.revoked.get(id.slot()).copied().flatten() == Some(id);
        let (a, b) = key.ids();
        gone(a)
            || b.is_some_and(gone)
            || matches!(key, Key::Wall(_, owner, ..) if self.evicted.contains(&owner))
    }
    /// Last step's force, projected onto the current normal.
    pub(crate) fn force(&self, key: Key, normal: Vec2) -> f32 {
        self.find(key, normal).unwrap_or(0.0)
    }
    /// The remembered force if this contact was recorded at all.
    pub(crate) fn find(&self, key: Key, normal: Vec2) -> Option<f32> {
        let mask = self.index.len().checked_sub(1)?;
        let mut at = bucket(key, mask);
        loop {
            let row = self.index[at];
            if row == NONE {
                return None;
            }
            let old = self.accepted[row as usize];
            if old.key == key {
                // A live key never matches a revoked identity, so no stale
                // check is needed here.
                return Some(old.force * old.normal.dot(normal).max(0.0));
            }
            at = (at + 1) & mask;
        }
    }
    pub(crate) fn begin(&mut self) {
        self.next.clear();
    }
    pub(crate) fn record(&mut self, contact: Contact) {
        if contact.force > 0.0 {
            self.next.push(contact);
        }
    }
    /// Record even a zero force, so a lookup knows the contact was solved.
    pub(crate) fn record_any(&mut self, contact: Contact) {
        self.next.push(contact);
    }
    /// Replace the memory with this step's contacts, in generation order.
    pub(crate) fn accept(&mut self) {
        core::mem::swap(&mut self.accepted, &mut self.next);
        for slot in self.revoked_slots.drain(..) {
            self.revoked[slot] = None;
        }
        self.evicted.clear();
        // At most half full, so probe runs stay short.
        let size = (2 * self.accepted.len()).next_power_of_two().max(16);
        self.index.clear();
        self.index.resize(size, NONE);
        let mask = size - 1;
        for (row, contact) in self.accepted.iter().enumerate() {
            let mut at = bucket(contact.key, mask);
            while self.index[at] != NONE {
                at = (at + 1) & mask;
            }
            self.index[at] = row as u32;
        }
    }
    pub(crate) fn revoke(&mut self, id: EntityId) {
        let slot = id.slot();
        if self.revoked.len() <= slot {
            self.revoked.resize(slot + 1, None);
        }
        if self.revoked[slot].is_none() {
            self.revoked_slots.push(slot);
        }
        self.revoked[slot] = Some(id);
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.evicted.push(owner);
    }
    /// Forget every force; the next step starts cold.
    pub(crate) fn clear(&mut self) {
        self.next.clear();
        self.accept();
    }
    fn live(&self) -> impl Iterator<Item = &Contact> {
        self.accepted.iter().filter(|c| !self.stale(c.key))
    }
    pub(crate) fn hash(&self, h: &mut Fnv) {
        h.usize(self.live().count());
        for entry in self.live() {
            entry.key.hash(h);
            h.f32(entry.normal.x);
            h.f32(entry.normal.y);
            h.f32(entry.force);
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("contact_forces", |out| {
            for entry in self.live() {
                out.object(&entry.key.to_string(), |out| {
                    out.num("force", entry.force);
                    out.num("normal_x", entry.normal.x);
                    out.num("normal_z", entry.normal.y);
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(memory: &Memory) -> u64 {
        let mut h = Fnv::default();
        memory.hash(&mut h);
        h.finish()
    }

    #[test]
    fn lookup_projection_revocation_and_eviction_follow_live_contacts() {
        let mut slots = crate::slots::Slots::default();
        let ids: Vec<_> = (0..65).map(|_| slots.insert()).collect();
        let mut world = crate::World::empty();
        let owner = world.load_scene(&crate::Scene::default()).unwrap();
        let mut memory = Memory::default();
        memory.begin();
        for (index, &id) in ids.iter().enumerate().skip(1) {
            memory.record(Contact {
                key: Key::bodies(ids[0], id),
                normal: Vec2::X,
                force: index as f32,
            });
            memory.record(Contact {
                key: Key::Wall(id, owner, index, 0),
                normal: Vec2::Y,
                force: 1.0,
            });
        }
        memory.record(Contact {
            key: Key::Arena(ids[3], 1),
            normal: Vec2::Y,
            force: 0.0,
        });
        memory.accept();
        assert_eq!(memory.force(Key::bodies(ids[9], ids[0]), Vec2::X), 9.0);
        assert_eq!(
            memory.force(Key::bodies(ids[0], ids[9]), Vec2::new(0.6, 0.8)),
            9.0 * 0.6
        );
        assert_eq!(memory.force(Key::bodies(ids[0], ids[9]), -Vec2::X), 0.0);
        assert_eq!(
            memory.force(Key::Arena(ids[3], 1), Vec2::Y),
            0.0,
            "zero force is not stored"
        );

        let before = digest(&memory);
        memory.revoke(ids[17]);
        slots.remove(ids[17]).unwrap();
        let replacement = slots.insert();
        assert_eq!(replacement.slot(), ids[17].slot());
        assert_eq!(memory.force(Key::bodies(ids[0], replacement), Vec2::X), 0.0);
        assert_ne!(
            digest(&memory),
            before,
            "a removed body's contacts leave the state"
        );
        let mut report = Report::default();
        memory.report(&mut report);
        assert!(!report.finish().contains(&ids[17].to_string()));

        memory.evict(owner);
        let mut report = Report::default();
        memory.report(&mut report);
        assert!(!report.finish().contains("wall"));
        let filtered = digest(&memory);
        // Rewriting from the same live contacts matches the filtered view.
        let live: Vec<_> = memory.live().copied().collect();
        memory.begin();
        for contact in live {
            memory.record(contact);
        }
        memory.accept();
        assert_eq!(digest(&memory), filtered);
        memory.clear();
        assert_eq!(memory.force(Key::bodies(ids[0], ids[9]), Vec2::X), 0.0);
    }
}
