//! Optional diagnostic boundaries. Observers receive facts, never mutable state.

/// Work performed by one crowd separation pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CrowdWork {
    pub(crate) candidates: u64,
    pub(crate) possible_pairs: u64,
    pub(crate) contacts: usize,
}

impl CrowdWork {
    /// Pairs sent to the exact disc test, each at most once in solver order.
    pub fn candidates(self) -> u64 { self.candidates }
    /// All unordered non-player pairs, including props, before spatial filtering.
    pub fn possible_pairs(self) -> u64 { self.possible_pairs }
    /// Pairs for which the exact contact test succeeded.
    pub fn contacts(self) -> usize { self.contacts }
}

/// Read-only phase notifications. Implement timing outside the simulation;
/// callbacks cannot supply decisions or access the world being stepped.
/// The default observer has no work and no clock dependency.
pub trait StepObserver {
    /// Immediately before crowd indexing and separation.
    fn crowd_started(&mut self) {}
    /// Immediately after separation, including aggregate trace emission.
    fn crowd_finished(&mut self, _work: CrowdWork) {}
}

impl StepObserver for () {}
