//! Sparse behaviour membership keyed by existing entity identities.
//!
//! Unlike [`crate::slots`], this does not allocate identities. Each behaviour
//! iterates only its own members; payload arrays remain with the behaviour owner.

use crate::hash::Fnv;
use crate::slots::{EntityId, VACANT};

/// Membership only; the behaviour owner keeps its payload arrays aligned.
#[derive(Default)]
pub(crate) struct Members {
    /// Entity slot to dense row, or [`VACANT`]. Indexed by
    /// [`EntityId::slot`], so it is as long as the highest slot ever used and
    /// is mostly holes — which is the trade a sparse set makes: memory
    /// proportional to the entity count, time proportional to the membership.
    sparse: Vec<u32>,
    /// Dense row to the id that owns it. Packed, so a pass streams it.
    ///
    /// It stores the **whole** id, generation included, and that is what makes
    /// a lookup safe. A stale id shares a slot with whoever recycled it, so
    /// finding a row proves nothing; the row has to name the asker back.
    dense: Vec<EntityId>,
}

impl Members {
    /// How many entities have this behaviour.
    pub(crate) fn len(&self) -> usize {
        self.dense.len()
    }

    /// The members, in storage order. What a pass iterates.
    pub(crate) fn ids(&self) -> &[EntityId] {
        &self.dense
    }

    /// Adds membership and returns the payload row to append.
    /// `None` means already present: do not append a payload row.
    pub(crate) fn add(&mut self, id: EntityId) -> Option<usize> {
        if self.index(id).is_some() {
            return None;
        }

        // Grow to cover this slot. Holes are `VACANT`, never left uninitialised
        // — an unwritten entry would read as a row that exists.
        if self.sparse.len() <= id.slot() {
            self.sparse.resize(id.slot() + 1, VACANT);
        }

        let row = self.dense.len();
        self.sparse[id.slot()] = row as u32;
        self.dense.push(id);
        Some(row)
    }

    /// Removes membership and returns the payload row to swap-remove, if present.
    pub(crate) fn remove(&mut self, id: EntityId) -> Option<usize> {
        let row = self.index(id)?;

        self.dense.swap_remove(row);
        if let Some(moved) = self.dense.get(row).copied() {
            self.sparse[moved.slot()] = row as u32;
        }
        self.sparse[id.slot()] = VACANT;

        Some(row)
    }

    /// Resolves the complete ID, including generation, to its dense row.
    pub(crate) fn index(&self, id: EntityId) -> Option<usize> {
        let row = *self.sparse.get(id.slot())? as usize;
        (self.dense.get(row) == Some(&id)).then_some(row)
    }

    /// Whether this entity has the behaviour.
    pub(crate) fn contains(&self, id: EntityId) -> bool {
        self.index(id).is_some()
    }

    /// Hashes dense identities only. `sparse` is a derived reverse index; hashing it
    /// would make cost proportional to all entity slots rather than this membership.
    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { sparse, dense } = self;
        let _ = sparse;

        h.usize(dense.len());
        for id in dense {
            id.hash_into(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slots::Slots;

    /// Membership beside a payload, which is how a behaviour with data uses
    /// this. The payload is what makes a mis-mapped row visible: without it,
    /// every wrong answer is still a plausible-looking index.
    #[derive(Default)]
    struct Behaviour {
        who: Members,
        data: Vec<&'static str>,
    }

    impl Behaviour {
        fn grant(&mut self, id: EntityId, what: &'static str) -> bool {
            let Some(row) = self.who.add(id) else { return false };
            self.data.push(what);
            assert_eq!(row, self.data.len() - 1, "add must hand back the row just pushed");
            assert_eq!(self.who.len(), self.data.len());
            true
        }

        fn revoke(&mut self, id: EntityId) -> bool {
            let Some(row) = self.who.remove(id) else { return false };
            self.data.swap_remove(row);
            assert_eq!(self.who.len(), self.data.len());
            true
        }

        fn get(&self, id: EntityId) -> Option<&'static str> {
            self.who.index(id).map(|row| self.data[row])
        }
    }

    #[test]
    fn a_member_finds_its_own_row() {
        let mut slots = Slots::default();
        let (a, b) = (slots.insert(), slots.insert());

        let mut behaviour = Behaviour::default();
        assert!(behaviour.grant(a, "a"));
        assert!(behaviour.grant(b, "b"));

        assert_eq!(behaviour.get(a), Some("a"));
        assert_eq!(behaviour.get(b), Some("b"));
    }

    /// Only members. The whole point of a behaviour being a set is that most
    /// entities are not in it.
    #[test]
    fn a_non_member_is_not_found() {
        let mut slots = Slots::default();
        let (member, bystander) = (slots.insert(), slots.insert());

        let mut behaviour = Behaviour::default();
        behaviour.grant(member, "member");

        assert!(!behaviour.who.contains(bystander));
        assert_eq!(behaviour.get(bystander), None);
        assert_eq!(behaviour.who.len(), 1, "a lookup must not have added anybody");
    }

    /// Removing from the middle swaps the last member down, so a survivor's row
    /// changes without anything touching it — the same hazard the body storage
    /// has, and the reason the sparse entry is repointed on the way out.
    #[test]
    fn a_survivor_keeps_its_data_when_a_swap_moves_its_row() {
        let mut slots = Slots::default();
        let (a, b, c) = (slots.insert(), slots.insert(), slots.insert());

        let mut behaviour = Behaviour::default();
        behaviour.grant(a, "a");
        behaviour.grant(b, "b");
        behaviour.grant(c, "c");

        assert!(behaviour.revoke(b));

        assert_eq!(behaviour.get(a), Some("a"));
        assert_eq!(behaviour.get(c), Some("c"), "the swapped member lost its data");
        assert_eq!(behaviour.get(b), None);
    }

    /// An unreclaimed dead membership must not resolve for a new generation.
    /// The old ID remains in this set until revoked; World owns that cleanup.
    #[test]
    fn a_recycled_slot_does_not_inherit_the_behaviour() {
        let mut slots = Slots::default();
        let doomed = slots.insert();

        let mut behaviour = Behaviour::default();
        behaviour.grant(doomed, "doomed");

        slots.remove(doomed);
        let newcomer = slots.insert();
        assert_eq!(newcomer.slot(), doomed.slot(), "the slot must be reused, or this proves nothing");

        assert!(!behaviour.who.contains(newcomer), "the newcomer inherited a behaviour");
        assert_eq!(behaviour.get(newcomer), None);
    }

    /// Granting twice must not push a second payload row, or the arrays drift
    /// apart and every lookup after it is off by one.
    #[test]
    fn granting_twice_is_a_no_op() {
        let mut slots = Slots::default();
        let id = slots.insert();

        let mut behaviour = Behaviour::default();
        assert!(behaviour.grant(id, "first"));
        assert!(!behaviour.grant(id, "second"), "the second grant reported success");

        assert_eq!(behaviour.who.len(), 1);
        assert_eq!(behaviour.get(id), Some("first"));
    }

    #[test]
    fn revoking_from_a_non_member_does_nothing() {
        let mut slots = Slots::default();
        let (member, bystander) = (slots.insert(), slots.insert());

        let mut behaviour = Behaviour::default();
        behaviour.grant(member, "member");

        assert!(!behaviour.revoke(bystander));
        assert_eq!(behaviour.get(member), Some("member"));
    }

    /// A membership set is sized by the entity slots it has seen, not by the
    /// horde: a high-numbered entity joining must not read off the end of the
    /// sparse array, and a set that has never seen a slot must answer `None`
    /// rather than panic.
    #[test]
    fn a_slot_beyond_the_sparse_array_is_not_a_member() {
        let mut slots = Slots::default();
        let mut far = slots.insert();
        for _ in 0..64 {
            far = slots.insert();
        }

        let behaviour = Behaviour::default();
        assert_eq!(behaviour.get(far), None, "an unseen slot must not index out of bounds");
    }

    /// Churn: grants and revokes interleaved, with the correspondence checked
    /// after every round. Fixed sequence, because a simulation test that cannot
    /// be replayed is not a test.
    #[test]
    fn membership_survives_repeated_churn() {
        let mut slots = Slots::default();
        let mut behaviour = Behaviour::default();
        let names = ["a", "b", "c", "d", "e", "f", "g"];
        let mut live: Vec<(EntityId, &'static str)> = Vec::new();

        for round in 0..8 {
            for name in &names {
                let id = slots.insert();
                behaviour.grant(id, name);
                live.push((id, name));
            }

            let stride = round % 3 + 2;
            let mut kept = Vec::new();
            for (i, entry) in live.drain(..).enumerate() {
                if i % stride == 0 {
                    assert!(behaviour.revoke(entry.0));
                    slots.remove(entry.0);
                } else {
                    kept.push(entry);
                }
            }
            live = kept;

            for (id, name) in &live {
                assert_eq!(behaviour.get(*id), Some(*name), "{id} lost its row in round {round}");
            }
            assert_eq!(behaviour.who.len(), live.len());
        }
    }

    /// Membership is state the determinism gate has to see: two worlds whose
    /// bodies stand in identical places are different worlds if different ones
    /// of them chase.
    #[test]
    fn different_membership_hashes_differently() {
        let mut slots = Slots::default();
        let (a, b) = (slots.insert(), slots.insert());

        let hash = |m: &Members| {
            let mut h = Fnv::default();
            m.hash(&mut h);
            h.finish()
        };

        let mut just_a = Members::default();
        just_a.add(a);

        let mut just_b = Members::default();
        just_b.add(b);

        let mut both = Members::default();
        both.add(a);
        both.add(b);

        assert_eq!(just_a.len(), just_b.len(), "same size, or this proves nothing");
        assert_ne!(hash(&just_a), hash(&just_b));
        assert_ne!(hash(&just_a), hash(&both));
    }
}
