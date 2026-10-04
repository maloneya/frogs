//! Generational body identities mapped to dense rows.
//!
//! Swap-removal moves rows, so retained references use [`EntityId`], not indices.
//! [`Slots`] owns identity only; body storage owns the parallel payload arrays.

use crate::hash::Fnv;

/// Stable body identity until despawn. Use `Option<EntityId>` for an absent target.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EntityId {
    /// Which slot. Recycled after a despawn.
    index: u32,
    /// Which occupant of that slot. Bumped every time it is vacated.
    generation: u32,
}

impl EntityId {
    /// Read-only diagnostic identity. This pair cannot construct a live handle.
    #[must_use]
    pub fn diagnostic_identity(self) -> (u32, u32) {
        (self.index, self.generation)
    }

    /// Sparse-table index only; generation must also match when resolving an identity.
    pub(crate) fn slot(self) -> usize {
        self.index as usize
    }

    /// Feeds the whole name into the determinism hash.
    pub fn hash_into(self, h: &mut Fnv) {
        h.u64(u64::from(self.index));
        h.u64(u64::from(self.generation));
    }
}

impl core::fmt::Display for EntityId {
    /// `#12v3` — slot 12, third occupant. Compact because it appears in trace
    /// events and failure messages, where the surrounding text is what carries
    /// the meaning.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "#{}v{}", self.index, self.generation)
    }
}

/// The generation a freshly occupied slot gets.
///
/// One rather than zero, so that generation 0 is never live. That is what makes
/// an all-zero `EntityId` — from zeroed memory, a bad deserialisation, a
/// forgotten initialisation — dead rather than a valid reference to slot 0.
const FIRST_GENERATION: u32 = 1;
const _: () = assert!(
    FIRST_GENERATION > 0,
    "generation 0 must stay dead, or a zeroed id names a real body"
);

/// Marks a slot with no body in it. `u32::MAX` rather than an `Option<u32>`
/// because this is one word in a table walked on every lookup, and the niche is
/// free: a dense index that large is unreachable long before it is representable
/// — the instance budget caps the world at a few thousand bodies.
pub(crate) const VACANT: u32 = u32::MAX;

/// One entry in the sparse table, indexed by [`EntityId::index`].
#[derive(Clone, Copy)]
struct Slot {
    /// Which occupant this slot is on. Compared against an id's generation to
    /// decide whether that id is still talking about the body it was minted for.
    generation: u32,
    /// Where this body's row currently lives, or [`VACANT`].
    dense: u32,
}

/// Maps stable IDs to dense rows. Payload owners must keep parallel arrays aligned;
/// this type cannot enforce their lengths.
#[derive(Default)]
pub(crate) struct Slots {
    /// Indexed by slot number. Grows and is never shrunk: a slot must keep its
    /// generation after the body in it dies, or recycling it would hand out an
    /// id indistinguishable from one already in circulation.
    slots: Vec<Slot>,
    /// Dense index to id, so a swap-remove can find the slot of the row it
    /// moved. Exactly as long as the caller's payload arrays.
    dense: Vec<EntityId>,
    /// Slots free for reuse, most recently freed first.
    ///
    /// A stack rather than a queue, deliberately. It is warmer in cache, and
    /// the alternative reason to prefer a queue — delaying reuse so stale ids
    /// stay detectable for longer — is a property generations already provide
    /// outright, so paying for it twice buys nothing.
    free: Vec<u32>,
}

impl Slots {
    /// How many bodies are live. The caller's payload arrays must be this long.
    pub(crate) fn len(&self) -> usize {
        self.dense.len()
    }

    /// Allocates an identity. The owner must append one row to every payload array.
    /// `Bodies::spawn` owns this pairing and checks lengths in debug builds.
    pub(crate) fn insert(&mut self) -> EntityId {
        let dense = self.dense.len() as u32;

        // Reserve retirement storage while constructing bodies. Defeat can then
        // return every live slot without allocating during a simulation tick.
        if self.free.capacity() < self.slots.len() + 1 {
            self.free.reserve(self.slots.len() + 1 - self.free.len());
        }
        let index = match self.free.pop() {
            Some(index) => {
                // Reused. The generation was already bumped on the way out, so
                // the previous occupant's id is dead and stays dead.
                self.slots[index as usize].dense = dense;
                index
            }
            None => {
                let index = self.slots.len() as u32;
                self.slots.push(Slot {
                    generation: FIRST_GENERATION,
                    dense,
                });
                index
            }
        };

        let id = EntityId {
            index,
            generation: self.slots[index as usize].generation,
        };
        self.dense.push(id);
        id
    }

    /// Retires an identity and returns the row to swap-remove from every payload array.
    /// Returns `None` for an already-dead ID; no payload row may be removed then.
    pub(crate) fn remove(&mut self, id: EntityId) -> Option<usize> {
        let dense = self.index(id)?;

        // The row that is about to be moved into the hole, if it is not the
        // hole itself.
        self.dense.swap_remove(dense);
        if let Some(moved) = self.dense.get(dense).copied() {
            self.slots[moved.index as usize].dense = dense as u32;
        }

        self.retire(id.index);
        Some(dense)
    }

    /// Marks a slot vacant, bumps past its occupant, and offers it for reuse.
    ///
    /// Every body removal calls this.
    /// The slot table survives: resetting generations would resurrect stale ids.
    fn retire(&mut self, index: u32) {
        let slot = &mut self.slots[index as usize];
        slot.dense = VACANT;

        // Wrapping, not saturating. At u32 this needs four billion despawns of
        // one slot before an ancient id could collide with a live one, which at
        // 60Hz is not reachable; saturating would instead freeze the generation
        // and make every future id for this slot collide, turning an
        // unreachable bug into a permanent one.
        slot.generation = slot.generation.wrapping_add(1);
        if slot.generation == 0 {
            // Skip the dead generation, so the zeroed-id invariant survives a
            // wrap.
            slot.generation = FIRST_GENERATION;
        }

        self.free.push(index);
    }

    /// Where this body's row lives, or `None` if the id is stale.
    ///
    /// The generation check is what makes a dangling reference return nothing
    /// instead of a stranger. The vacancy check is redundant while generations
    /// behave — a vacated slot has already been bumped past every id that named
    /// it — and is kept because it is one comparison and it is what holds if
    /// the generation ever wraps.
    pub(crate) fn index(&self, id: EntityId) -> Option<usize> {
        let slot = self.slots.get(id.index as usize)?;
        (slot.generation == id.generation && slot.dense != VACANT).then_some(slot.dense as usize)
    }

    /// Whether this id still names a live body.
    pub(crate) fn contains(&self, id: EntityId) -> bool {
        self.index(id).is_some()
    }

    /// Every live name, in storage order.
    ///
    /// For granting a behaviour in bulk. Deliberately a slice rather than a
    /// way to *reach* a body: it hands out names, and a name still has to be
    /// resolved through the same door as any other.
    pub(crate) fn ids(&self) -> &[EntityId] {
        &self.dense
    }

    /// Hashes allocator state, including generations and free-list order: these
    /// influence the identities of future spawns even when current positions agree.
    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { slots, dense, free } = self;

        h.usize(slots.len());
        for slot in slots {
            h.u64(u64::from(slot.generation));
            h.u64(u64::from(slot.dense));
        }

        h.usize(dense.len());
        for id in dense {
            id.hash_into(h);
        }

        h.usize(free.len());
        for index in free {
            h.u64(u64::from(*index));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Models how a caller actually uses this: one `Slots` beside one payload
    /// array, kept the same length. Every test below drives the pair, because
    /// the bugs worth catching here are all bugs in the *correspondence*, not
    /// in either half alone.
    #[derive(Default)]
    struct Store {
        slots: Slots,
        payload: Vec<&'static str>,
    }

    impl Store {
        fn spawn(&mut self, what: &'static str) -> EntityId {
            let id = self.slots.insert();
            self.payload.push(what);
            assert_eq!(
                self.slots.len(),
                self.payload.len(),
                "the pairing contract broke"
            );
            id
        }

        fn despawn(&mut self, id: EntityId) -> bool {
            let Some(dense) = self.slots.remove(id) else {
                return false;
            };
            self.payload.swap_remove(dense);
            assert_eq!(
                self.slots.len(),
                self.payload.len(),
                "the pairing contract broke"
            );
            true
        }

        fn get(&self, id: EntityId) -> Option<&'static str> {
            self.slots.index(id).map(|i| self.payload[i])
        }
    }

    #[test]
    fn a_fresh_id_finds_its_own_row() {
        let mut store = Store::default();
        let a = store.spawn("a");
        let b = store.spawn("b");

        assert_eq!(store.get(a), Some("a"));
        assert_eq!(store.get(b), Some("b"));
    }

    /// **The point of the whole type.** Removing a body from the middle swaps
    /// the last one into its row — so the survivor's index changes without
    /// anything touching it, and its id must follow.
    ///
    /// A plain index would now name the wrong body, and nothing would say so:
    /// the read succeeds and returns a stranger.
    #[test]
    fn a_survivor_keeps_its_identity_when_a_swap_moves_its_row() {
        let mut store = Store::default();
        let a = store.spawn("a");
        let b = store.spawn("b");
        let c = store.spawn("c");

        // Removing the middle row moves `c` into it.
        assert!(store.despawn(b));

        assert_eq!(store.get(a), Some("a"));
        assert_eq!(
            store.get(c),
            Some("c"),
            "the swapped body's id followed its data"
        );
        assert_eq!(store.get(b), None, "a despawned id must not resolve");
    }

    /// The ABA case, and the reason ids carry a generation at all. The slot is
    /// recycled; the dead id must not come back to life pointing at whoever
    /// moved in.
    #[test]
    fn a_recycled_slot_does_not_revive_the_id_that_used_to_own_it() {
        let mut store = Store::default();
        let old = store.spawn("old");
        assert!(store.despawn(old));

        let new = store.spawn("new");

        assert_eq!(
            new.index, old.index,
            "the slot should be reused, or this proves nothing"
        );
        assert_ne!(new.generation, old.generation);
        assert_eq!(store.get(new), Some("new"));
        assert_eq!(
            store.get(old),
            None,
            "a stale id resolved to its slot's new occupant"
        );
    }

    /// Despawning twice must be a no-op rather than corrupting the free list
    /// with a duplicate — which would later hand the same slot to two live
    /// bodies at once.
    #[test]
    fn despawning_an_already_dead_id_does_nothing() {
        let mut store = Store::default();
        let a = store.spawn("a");
        let b = store.spawn("b");

        assert!(store.despawn(a));
        assert!(!store.despawn(a), "a second despawn reported success");

        assert_eq!(store.get(b), Some("b"));
        assert_eq!(store.slots.len(), 1);

        // The slot must be handed out exactly once despite the double despawn.
        let c = store.spawn("c");
        let d = store.spawn("d");
        assert_ne!(
            store.slots.index(c),
            store.slots.index(d),
            "one slot, two live bodies"
        );
    }

    /// A zeroed id — from a placeholder, or memory that was never written —
    /// must be dead rather than naming slot zero. This is what
    /// [`FIRST_GENERATION`] buys, and it is only testable from inside the
    /// module because the fields are private everywhere else.
    #[test]
    fn a_zeroed_id_names_nothing() {
        let mut store = Store::default();
        store.spawn("a");

        assert_eq!(
            store.get(EntityId {
                index: 0,
                generation: 0
            }),
            None
        );
    }

    /// An id from a different store must not resolve here just because the
    /// numbers line up. It is the same failure as a stale id and it happens for
    /// real once there is more than one array keyed by entity.
    #[test]
    fn an_index_past_the_end_is_dead_rather_than_a_panic() {
        let store = Store::default();
        assert_eq!(
            store.get(EntityId {
                index: 7,
                generation: 1
            }),
            None
        );
    }

    /// Churn: the pairing must survive an interleaving of spawns and despawns,
    /// not just one of each. Fixed sequence rather than random, because a
    /// simulation test that cannot be replayed is not a test.
    #[test]
    fn identity_survives_repeated_churn() {
        let mut store = Store::default();
        let mut live: Vec<(EntityId, &'static str)> = Vec::new();
        let names = ["a", "b", "c", "d", "e", "f", "g", "h"];

        for round in 0..8 {
            for name in &names {
                live.push((store.spawn(name), name));
            }
            // Remove a strided subset, so the swap target varies rather than
            // always being the row next door.
            let stride = round % 3 + 2;
            let mut kept = Vec::new();
            for (i, entry) in live.drain(..).enumerate() {
                if i % stride == 0 {
                    assert!(store.despawn(entry.0));
                } else {
                    kept.push(entry);
                }
            }
            live = kept;

            for (id, name) in &live {
                assert_eq!(
                    store.get(*id),
                    Some(*name),
                    "{id} lost its row in round {round}"
                );
            }
            assert_eq!(store.slots.len(), live.len());
        }
    }

    /// Two stores driven the same way must hash the same, or the determinism
    /// gate would report a difference that is not one.
    #[test]
    fn the_same_history_hashes_the_same() {
        let history = |extra: bool| {
            let mut store = Store::default();
            let a = store.spawn("a");
            store.spawn("b");
            store.despawn(a);
            store.spawn("c");
            if extra {
                store.spawn("d");
            }
            let mut h = Fnv::default();
            store.slots.hash(&mut h);
            h.finish()
        };

        assert_eq!(history(false), history(false));
        assert_ne!(history(false), history(true));
    }

    /// **Spawn history is state, even when it is invisible.** Two worlds with
    /// the same live bodies but different histories hand out different ids
    /// next, so a hash that called them equal would let a real divergence go
    /// unreported for a tick and then blame the wrong pass for it.
    #[test]
    fn the_same_live_set_reached_two_ways_hashes_differently() {
        let mut fresh = Slots::default();
        fresh.insert();

        let mut recycled = Slots::default();
        let doomed = recycled.insert();
        recycled.remove(doomed);
        recycled.insert();

        assert_eq!(
            fresh.len(),
            recycled.len(),
            "both hold one body, or this proves nothing"
        );

        let hash = |s: &Slots| {
            let mut h = Fnv::default();
            s.hash(&mut h);
            h.finish()
        };
        assert_ne!(hash(&fresh), hash(&recycled));
    }
}
