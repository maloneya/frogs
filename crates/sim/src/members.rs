//! Which entities have a behaviour, and where each one's data sits.
//!
//! **The other half of the storage decision, and a different job from
//! [`crate::slots`].** `Slots` is the entity allocator: it *mints* names and
//! owns the bodies every entity has. This attaches to a name that already
//! exists. A body is spawned once; the behaviours it carries come and go, and
//! most entities do not carry most behaviours.
//!
//! ## Why a behaviour owns its own membership
//!
//! The alternative is one wide table with a flag per behaviour, and it fails in
//! the two ways that matter here. Every body pays for every behaviour whether it
//! has it or not, so the table grows into exactly the god struct this project
//! is trying not to write. And a pass over it costs the size of the *horde*
//! rather than the size of its own membership: three chasers among a thousand
//! bodies would still be a thousand iterations and a thousand branches.
//!
//! Here, a behaviour is its own storage plus a pass that reads it. Adding one
//! touches no existing type, and it costs what it uses.
//!
//! ## The layout
//!
//! A sparse set. `sparse` is indexed by an entity's *slot* and points into
//! `dense`; `dense` is packed, so a pass streams it with no holes and no
//! branches. Membership is O(1) to test, add and remove, and iteration is
//! sequential — which are the two things a behaviour actually does.
//!
//! Removal swaps the last row down, exactly as the body storage does, so a
//! member's row moves without anything touching that member. That is the same
//! hazard `EntityId` exists for, and the reason a lookup validates the whole id
//! rather than just finding a row.

use crate::hash::Fnv;
use crate::slots::EntityId;

/// Marks a slot belonging to no member. See `slots::VACANT` for why a sentinel
/// rather than an `Option`.
const VACANT: u32 = u32::MAX;

/// The entities that have one behaviour.
///
/// Holds no payload, for the same reason [`crate::slots::Slots`] holds none:
/// the owner keeps its own dense arrays alongside and pushes to them in step.
/// One membership set can then serve a behaviour with no data at all, one with
/// a single `f32`, or one with five parallel arrays.
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

    /// Grants the behaviour, and says which dense row to push payload at.
    ///
    /// Returns `None` if the entity already had it — in which case the caller
    /// must **not** push, or its arrays would drift a row longer than this.
    /// Granting twice is not an error worth refusing loudly: it is what a
    /// spawner does when it applies a template to a body that already matches.
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

    /// Revokes the behaviour, and says which dense row to `swap_remove`.
    ///
    /// `None` if the entity did not have it. As in `Slots::remove`, the row that
    /// was last is moved into the hole and its owner's sparse entry is repointed
    /// here, so the caller only has to `swap_remove` its own arrays at the
    /// returned index.
    pub(crate) fn remove(&mut self, id: EntityId) -> Option<usize> {
        let row = self.index(id)?;

        self.dense.swap_remove(row);
        if let Some(moved) = self.dense.get(row).copied() {
            self.sparse[moved.slot()] = row as u32;
        }
        self.sparse[id.slot()] = VACANT;

        Some(row)
    }

    /// Which dense row belongs to this entity, or `None` if it is not a member.
    ///
    /// **Both halves of the check are load-bearing.** Finding a row for the slot
    /// says only that *somebody* with that slot is a member; comparing the whole
    /// id says it is this one. Without the second half, an id retired and its
    /// slot recycled would read as a member — and would read as a member with
    /// somebody else's data.
    pub(crate) fn index(&self, id: EntityId) -> Option<usize> {
        let row = *self.sparse.get(id.slot())? as usize;
        (row != VACANT as usize && self.dense.get(row) == Some(&id)).then_some(row)
    }

    /// Whether this entity has the behaviour.
    pub(crate) fn contains(&self, id: EntityId) -> bool {
        self.index(id).is_some()
    }

    /// Revokes the behaviour from everybody.
    pub(crate) fn clear(&mut self) {
        self.sparse.clear();
        self.dense.clear();
    }

    /// Feeds membership into the world hash.
    ///
    /// Exhaustively destructured, as every hash in this crate is. `sparse` is
    /// derivable from `dense` and goes in anyway: the rule is every field, and
    /// an exception is how that rule stops being checkable.
    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self { sparse, dense } = self;

        h.usize(sparse.len());
        for entry in sparse {
            h.u64(u64::from(*entry));
        }

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

    /// **Why a lookup compares the whole id and not just the slot.** A dead
    /// entity's slot is recycled; the newcomer never had the behaviour, and the
    /// dead member's row must not answer for it.
    ///
    /// Finding a row for the slot proves only that *somebody* with that slot
    /// belongs. Without the generation check this hands the dead member's data
    /// to a body that never had the behaviour at all — a body nobody asked to
    /// chase would start chasing.
    ///
    /// **What this deliberately does not assert** is that the dead id stops
    /// resolving. It does not, and that is correct: this type is a set keyed by
    /// id, it holds no reference to the allocator, and nobody has told it the
    /// entity died. An earlier version of this test asserted otherwise and
    /// failed — the expectation was wrong, not the code.
    ///
    /// Two things cover that gap, and both are tested where they live rather
    /// than wished for here: `World::despawn_enemy` revokes eagerly, and
    /// `pass::seek` resolves every member against the body storage and skips
    /// what is gone. The second is what makes the first an optimisation rather
    /// than a correctness requirement.
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
