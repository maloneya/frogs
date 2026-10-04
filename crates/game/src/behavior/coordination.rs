//! Running combat actions own reservations and determine the admission count.
//! A bounded pool reuses their strike histories; completion and cancellation
//! release the associated history without adding another admission counter.

use arpg_core::Report;
use arpg_sim::{EntityId, Fnv};

/// Bounded combat pressure; other actions never reserve these slots.
pub const MAX_COMBAT_COMMITMENTS: usize = 4;
const COMMIT_SPACING: u32 = 12;

#[derive(Default)]
pub(super) struct Coordination {
    cooldown: u32,
    next: usize,
    strikes: [StrikeHistory; MAX_COMBAT_COMMITMENTS],
}
impl Coordination {
    pub(super) fn begin_tick(&mut self) {
        self.cooldown = self.cooldown.saturating_sub(1);
    }
    pub(super) fn can_start(&self, committed: usize) -> bool {
        committed < MAX_COMBAT_COMMITMENTS && self.cooldown == 0
    }
    pub(super) fn start_index(&self) -> usize {
        self.next
    }
    pub(super) fn granted(&mut self, index: usize, count: usize, actor: EntityId) {
        assert!(
            self.strikes
                .iter()
                .all(|history| history.actor != Some(actor)),
            "a new combat commitment cannot inherit strike history"
        );
        let history = self
            .strikes
            .iter_mut()
            .find(|history| history.actor.is_none())
            .expect("combat admission reserved a strike history");
        assert!(history.ordered.is_empty());
        history.actor = Some(actor);
        self.cooldown = COMMIT_SPACING;
        self.next = (index + 1) % count;
    }
    pub(super) fn retain_count(&mut self, count: usize) {
        if count == 0 {
            self.cooldown = 0;
            self.next = 0;
        } else {
            self.next %= count;
        }
    }
    /// Running actions own reservations; discard their histories at the same boundary.
    pub(super) fn retain_strikes(&mut self, active: impl Fn(EntityId) -> bool) {
        for history in &mut self.strikes {
            if history.actor.is_some_and(|actor| !active(actor)) {
                history.clear();
            }
        }
    }
    pub(super) fn release_strikes(&mut self, actor: EntityId) {
        for history in &mut self.strikes {
            if history.actor == Some(actor) {
                history.clear();
            }
        }
    }
    pub(super) fn strike(&mut self, actor: EntityId, target: EntityId) -> bool {
        self.strikes
            .iter_mut()
            .find(|history| history.actor == Some(actor))
            .expect("combat action owns strike history")
            .insert(target)
    }
    pub(super) fn struck(&self, actor: EntityId) -> &[EntityId] {
        self.strikes
            .iter()
            .find(|history| history.actor == Some(actor))
            .map_or(&[], |history| history.ordered.as_slice())
    }
    // Histories are hashed/reported beside their owning actors. Pool placement
    // and sparse indices are derived storage, not additional gameplay state.
    pub(super) fn hash(&self, out: &mut Fnv) {
        out.usize(self.cooldown as usize);
        out.usize(self.next);
    }
    pub(super) fn report(&self, out: &mut Report) {
        out.int("commit_cooldown", u64::from(self.cooldown));
        out.int("next", self.next as u64);
    }
}

/// Four reusable sparse sets, independent of the number of actors that attack
/// over a session. Ordered identities retain deterministic hash/report order;
/// sparse indices only accelerate membership and never drive iteration.
#[derive(Default)]
struct StrikeHistory {
    actor: Option<EntityId>,
    sparse: Vec<usize>,
    ordered: Vec<EntityId>,
}
impl StrikeHistory {
    fn insert(&mut self, target: EntityId) -> bool {
        let slot = target.diagnostic_identity().0 as usize;
        if self.sparse.len() <= slot {
            self.sparse.resize(slot + 1, usize::MAX);
        }
        if self.ordered.get(self.sparse[slot]) == Some(&target) {
            return false;
        }
        self.sparse[slot] = self.ordered.len();
        self.ordered.push(target);
        true
    }
    fn clear(&mut self) {
        for target in self.ordered.drain(..) {
            self.sparse[target.diagnostic_identity().0 as usize] = usize::MAX;
        }
        self.actor = None;
    }
}
