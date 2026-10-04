//! Optional diagnostic boundaries. Observers receive facts, never mutable state.

/// Work performed by one shared crowd-physics pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CrowdWork {
    pub(crate) candidates: u64,
    pub(crate) possible_pairs: u64,
    pub(crate) contacts: usize,
}

impl CrowdWork {
    /// Crowd pairs sent to exact geometry tests in the tick's first collision pass.
    pub fn candidates(self) -> u64 {
        self.candidates
    }
    /// All unordered non-player pairs, including props, in that same pass
    /// before spatial filtering.
    pub fn possible_pairs(self) -> u64 {
        self.possible_pairs
    }
    /// Non-player pairs overlapping at the start of the tick.
    pub fn contacts(self) -> usize {
        self.contacts
    }
}

/// Read-only phase notifications. Implement timing outside the simulation;
/// callbacks cannot supply decisions or access the world being stepped.
/// Each tick brackets one contact resolution pass. The default observer has
/// no work and no clock dependency.
pub trait StepObserver {
    /// Enable extra traversals used only for detailed work counts.
    const PHYSICS_DETAILS: bool = false;
    /// Switch exclusive timing attribution within the current crowd pass.
    fn physics_phase(&mut self, _phase: PhysicsPhase) {}
    /// Work performed, summed over the tick's collision passes and substeps.
    fn physics_count(&mut self, _counter: PhysicsCounter, _amount: u64) {}
    /// Immediately before a shared crowd-physics pass.
    fn crowd_started(&mut self) {}
    /// Immediately after that pass, including any aggregate trace emission.
    fn crowd_finished(&mut self, _work: CrowdWork) {}
}

impl StepObserver for () {}

/// Detailed physics timing phases; diagnostic only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsPhase {
    /// Bookkeeping not inside a named physics phase.
    Other,
    /// Rebuild the body grid.
    Broadphase,
    /// Exact body/body contact tests over grid pairs.
    CrowdContacts,
    /// Body/wall, body/arena, body/panel and panel/solid contact tests.
    SolidContacts,
    /// Effective masses, softness and warm-start lookup.
    Prepare,
    /// Reapply last substep's impulses.
    WarmStart,
    /// One soft, biased velocity iteration.
    Solve,
    /// Advance positions and panel angles.
    Integrate,
    /// One rigid, unbiased velocity iteration.
    Relax,
    /// Continuous safety sweeps for bodies beyond the speculative reach.
    Continuous,
    /// Store forces for the next step's warm start.
    Store,
}
impl PhysicsPhase {
    /// Stable report order.
    pub const ALL: [Self; 11] = [
        Self::Other,
        Self::Broadphase,
        Self::CrowdContacts,
        Self::SolidContacts,
        Self::Prepare,
        Self::WarmStart,
        Self::Solve,
        Self::Integrate,
        Self::Relax,
        Self::Continuous,
        Self::Store,
    ];
    /// Machine-readable field name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Other => "other",
            Self::Broadphase => "broadphase",
            Self::CrowdContacts => "crowd_contacts",
            Self::SolidContacts => "solid_contacts",
            Self::Prepare => "prepare",
            Self::WarmStart => "warm_start",
            Self::Solve => "solve",
            Self::Integrate => "integrate",
            Self::Relax => "relax",
            Self::Continuous => "continuous",
            Self::Store => "store",
        }
    }
}

/// Detailed physics work counters; diagnostic only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsCounter {
    /// Collision passes; more than one when fast bodies shorten the step.
    CollisionPasses,
    /// Velocity substeps across all collision passes.
    Substeps,
    /// Non-player grid candidate visits.
    Candidates,
    /// Body/body contact rows, summed over collision passes.
    BodyRows,
    /// Rows against walls, the arena edge or a panel, summed over passes.
    SolidRows,
    /// Rows whose surfaces were apart when collected.
    SpeculativeRows,
    /// Bodies swept by the continuous safety pass.
    ContinuousBodies,
    /// Latches that failed this tick.
    LatchBreaks,
}
impl PhysicsCounter {
    /// Stable report order.
    pub const ALL: [Self; 8] = [
        Self::CollisionPasses,
        Self::Substeps,
        Self::Candidates,
        Self::BodyRows,
        Self::SolidRows,
        Self::SpeculativeRows,
        Self::ContinuousBodies,
        Self::LatchBreaks,
    ];
    /// Machine-readable field name.
    pub fn label(self) -> &'static str {
        match self {
            Self::CollisionPasses => "collision_passes",
            Self::Substeps => "substeps",
            Self::Candidates => "candidates",
            Self::BodyRows => "body_rows",
            Self::SolidRows => "solid_rows",
            Self::SpeculativeRows => "speculative_rows",
            Self::ContinuousBodies => "continuous_bodies",
            Self::LatchBreaks => "latch_breaks",
        }
    }
}
