//! One-shot activation starts a scene-local source on the following tick.
//! This module owns the relationship and its consumed state, never enablement.

use arpg_core::Report;
use arpg_sim::{EntityId, Fnv, InteractionState, InteractionView, SceneId, SourceEnablement,
    SourceId, SourceState, TraceSink};
use serde::Deserialize;

/// Authored indices resolve once, against this scene's explicit bodies and sources.
/// Grid-generated bodies and descendants are deliberately not addressable here.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceControlSpec {
    /// Index in the engine scene's explicit bodies list; must be interactable.
    pub body: usize,
    /// Index in the engine scene's sources list; must start disabled.
    pub source: usize,
}

/// A relationship is consumed once, even if an external command later pauses its source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ControlPhase {
    /// Waiting for its live interaction endpoint to be activated.
    Pending,
    /// Applied its one enablement request.
    Started,
    /// An endpoint disappeared before the request could be applied.
    Orphaned,
}

/// Derived observation and the exact assertion vocabulary used by scenarios.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlState {
    /// Stored relationship phase.
    pub phase: ControlPhase,
    /// Current engine state, or None if its source has been removed.
    pub source: Option<SourceState>,
}

/// A gameplay transition. Diagnostic only; passes never consume the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlEvent {
    /// Owning scene instance.
    pub scene: SceneId,
    /// Authored connection index within the instance.
    pub control: usize,
    /// New relationship phase.
    pub phase: ControlPhase,
}

impl std::fmt::Display for ControlEvent {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { scene, control, phase } = self;
        write!(out, "control scene={scene} index={control} phase={phase:?}")
    }
}

struct Control {
    owner: SceneId,
    nth: usize,
    body: EntityId,
    source: SourceId,
    phase: ControlPhase,
}

/// Only scene installation may create resolved relationships.
#[derive(Default)]
pub(crate) struct SourceControls {
    rows: Vec<Control>,
}

impl SourceControls {
    pub(crate) fn is_empty(&self) -> bool { self.rows.is_empty() }
    pub(crate) fn install(&mut self, owner: SceneId, specs: &[SourceControlSpec],
        bodies: &[EntityId], sources: &[SourceId]) {
        for (nth, spec) in specs.iter().enumerate() {
            let SourceControlSpec { body, source } = *spec;
            self.rows.push(Control {
                owner, nth, body: bodies[body], source: sources[source], phase: ControlPhase::Pending,
            });
        }
    }

    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|row| row.owner != owner);
    }

    pub(crate) fn state(&self, owner: SceneId, nth: usize, world: &arpg_sim::World) -> Option<ControlState> {
        self.rows.iter().find(|row| row.owner == owner && row.nth == nth)
            .map(|row| ControlState { phase: row.phase, source: world.source_state(row.source) })
    }

    pub(crate) fn hash(&self, out: &mut Fnv) {
        let Self { rows } = self;
        out.usize(rows.len());
        for row in rows {
            let Control { owner, nth, body, source, phase } = row;
            owner.hash_into(out);
            out.usize(*nth);
            body.hash_into(out);
            source.hash_into(out);
            out.usize(match phase {
                ControlPhase::Pending => 0,
                ControlPhase::Started => 1,
                ControlPhase::Orphaned => 2,
            });
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { rows } = self;
        out.object("source_controls", |out| {
            for row in rows {
                let Control { owner, nth, body, source, phase } = row;
                out.object(&format!("{owner}/{nth}"), |out| {
                    out.text("body", &body.to_string());
                    out.text("source", &source.to_string());
                    out.text("phase", &format!("{phase:?}"));
                });
            }
        });
    }
}

/// Runs before the engine step, reading only the previous completed interaction
/// state. Activation on N is therefore consumed on N+1. Missing endpoints retire
/// pending records; completed records never reapply their request.
pub(crate) fn advance(controls: &mut SourceControls, interactions: InteractionView<'_>,
    mut sources: SourceEnablement<'_>, mut trace: TraceSink<'_, ControlEvent>) {
    for row in &mut controls.rows {
        if row.phase != ControlPhase::Pending { continue; }
        row.phase = match (interactions.state(row.body), sources.state(row.source)) {
            (None, _) | (_, None) => ControlPhase::Orphaned,
            (Some(InteractionState::Activated), Some(_)) => {
                assert!(sources.set(row.source, true), "source was resolved in this same exclusive borrow");
                ControlPhase::Started
            }
            (Some(InteractionState::Ready), Some(_)) => continue,
        };
        trace.emit(ControlEvent { scene: row.owner, control: row.nth, phase: row.phase });
    }
}
