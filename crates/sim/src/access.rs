//! Capability splits for deterministic callers outside the engine schedule.
//! The constructor can borrow World; the returned types cannot reach it.

use crate::{EntityId, InteractionState, SourceId, SourceState, TraceSink, World};
use crate::pass::interact::Interactions;
use crate::pass::source::Sources;

/// Read access to interaction membership without access to body storage.
pub struct InteractionView<'a>(&'a Interactions);

impl InteractionView<'_> {
    /// Missing or retired identities return None.
    #[must_use]
    pub fn state(&self, id: EntityId) -> Option<InteractionState> {
        self.0.get(id)
    }
}

/// Source enablement authority, with no spawn queue or physical storage access.
pub struct SourceEnablement<'a> {
    sources: &'a mut Sources,
    trace: TraceSink<'a>,
}

impl SourceEnablement<'_> {
    /// Reads the current source state.
    #[must_use]
    pub fn state(&self, id: SourceId) -> Option<SourceState> {
        self.sources.state(id)
    }

    /// Changes a live source's enablement; false means absent or retired.
    #[must_use]
    pub fn set(&mut self, id: SourceId, enabled: bool) -> bool {
        self.sources.set_enabled(id, enabled, self.trace.reborrow())
    }
}

impl World {
    /// Splits interaction observation from source enablement authority.
    /// Neither capability can move, damage, create, or remove a body.
    pub fn interaction_sources(&mut self) -> (InteractionView<'_>, SourceEnablement<'_>) {
        (InteractionView(&self.bodies.interactions),
         SourceEnablement { sources: &mut self.sources, trace: self.trace.sink(self.tick) })
    }
}
