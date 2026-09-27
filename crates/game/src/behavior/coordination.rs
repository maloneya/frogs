//! Shared admission policy. Running combat actions are the reservation owners;
//! deriving their count avoids a second list that could leak on cancellation.

use arpg_core::Report;
use arpg_sim::Fnv;

/// Bounded combat pressure; other actions never reserve these slots.
pub const MAX_COMBAT_COMMITMENTS: usize = 4;
const COMMIT_SPACING: u32 = 12;

#[derive(Default)]
pub(super) struct Coordination {
    cooldown: u32,
    next: usize,
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
    pub(super) fn granted(&mut self, index: usize, count: usize) {
        self.cooldown = COMMIT_SPACING;
        self.next = (index + 1) % count;
    }
    pub(super) fn retain_count(&mut self, count: usize) {
        if count == 0 {
            *self = Self::default();
        } else {
            self.next %= count;
        }
    }
    pub(super) fn hash(&self, out: &mut Fnv) {
        out.usize(self.cooldown as usize);
        out.usize(self.next);
    }
    pub(super) fn report(&self, out: &mut Report) {
        out.int("commit_cooldown", u64::from(self.cooldown));
        out.int("next", self.next as u64);
    }
}
