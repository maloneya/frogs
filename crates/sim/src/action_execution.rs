//! Shared tick lifecycle and control ownership for input and autonomous actions.
//! Producers choose operations; this owner defines their phase and retirement rules.
use crate::{AttackPhase, Fnv};

/// Validated finite startup, active and recovery windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionTimeline {
    startup: u32,
    active: u32,
    recovery: u32,
}

/// An operation either has a finite timeline or ends on an explicit result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionSchedule {
    /// Movement, interaction and held channels complete through their producer.
    Open,
    /// Validated half-open windows, constructed through [`Self::timed`].
    Timed(ActionTimeline),
}
impl ActionSchedule {
    /// Active duration must be positive; startup/recovery may be zero for waits.
    /// Complete duration is checked before an execution can be created.
    pub fn timed(startup: u32, active: u32, recovery: u32) -> Result<Self, &'static str> {
        if active == 0
            || startup
                .checked_add(active)
                .and_then(|n| n.checked_add(recovery))
                .is_none()
        {
            return Err("action needs positive active ticks and a representable total duration");
        }
        Ok(Self::Timed(ActionTimeline {
            startup,
            active,
            recovery,
        }))
    }
    fn duration(self) -> Option<u32> {
        match self {
            Self::Open => None,
            Self::Timed(t) => Some(t.startup + t.active + t.recovery),
        }
    }
    fn hash_into(self, hash: &mut Fnv) {
        match self {
            Self::Open => hash.usize(0),
            Self::Timed(ActionTimeline {
                startup,
                active,
                recovery,
            }) => {
                hash.usize(1);
                hash.u64(u64::from(startup));
                hash.u64(u64::from(active));
                hash.u64(u64::from(recovery));
            }
        }
    }
}

/// Controls reserved by an operation while it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionControls {
    /// No locomotion or facing ownership.
    None = 0,
    /// Owns the motion command; physical impulses still compose underneath.
    Movement = 1,
    /// Owns the facing command or committed direction.
    Facing = 2,
    /// Owns both motion and facing.
    MovementAndFacing = 3,
}

/// Whether a new producer decision may replace a running operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interruption {
    /// Ordinary decisions may replace this action immediately.
    Interruptible,
    /// Retains its controls until completion; invalid identity/removal can still cancel.
    Committed,
}

/// Current or retired outcome shared by every action producer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActionOutcome {
    /// No operation has started.
    #[default]
    None,
    /// Still owns its controls.
    Running,
    /// Completed its objective or finite timeline.
    Succeeded,
    /// Its objective or effect was refused.
    Failed,
    /// A replacement or invalid identity interrupted execution.
    Cancelled,
}

/// Captured execution state. Timing, ownership and outcome have one shared owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionExecution {
    schedule: ActionSchedule,
    controls: ActionControls,
    interruption: Interruption,
    elapsed: u32,
    outcome: ActionOutcome,
}
impl ActionExecution {
    /// Tick zero is the first executed tick; subsequent ticks advance before use.
    pub fn new(
        schedule: ActionSchedule,
        controls: ActionControls,
        interruption: Interruption,
    ) -> Self {
        Self {
            schedule,
            controls,
            interruption,
            elapsed: 0,
            outcome: ActionOutcome::Running,
        }
    }
    /// Advance before accepting this tick's decisions. Terminal actions never advance.
    pub fn advance(&mut self) {
        if !self.is_running() {
            return;
        }
        self.elapsed = self.elapsed.saturating_add(1);
        if self
            .schedule
            .duration()
            .is_some_and(|duration| self.elapsed >= duration)
        {
            self.complete(true);
        }
    }
    /// Explicit objective/effect result for a currently running operation.
    pub fn complete(&mut self, succeeded: bool) {
        if self.is_running() {
            self.outcome = if succeeded {
                ActionOutcome::Succeeded
            } else {
                ActionOutcome::Failed
            };
        }
    }
    /// Forced retirement, including invalid targets and body removal.
    pub fn cancel(&mut self) {
        if self.is_running() {
            self.outcome = ActionOutcome::Cancelled;
        }
    }
    /// Elapsed ticks since this operation began; open operations saturate at u32::MAX.
    pub fn elapsed(self) -> u32 {
        self.elapsed
    }
    /// Whether this action is still executing.
    pub fn is_running(self) -> bool {
        self.outcome == ActionOutcome::Running
    }
    /// Current execution outcome.
    pub fn outcome(self) -> ActionOutcome {
        self.outcome
    }
    /// Whether a producer decision can replace this operation now.
    pub fn interruptible(self) -> bool {
        !self.is_running() || self.interruption == Interruption::Interruptible
    }
    /// Whether the running action owns all requested controls.
    pub fn holds(self, requested: ActionControls) -> bool {
        self.is_running()
            && requested != ActionControls::None
            && (self.controls as u8 & requested as u8) == requested as u8
    }
    /// Control ownership after the next tick's retirement, before new input.
    pub fn holds_next_tick(self, requested: ActionControls) -> bool {
        self.holds(requested)
            && self
                .schedule
                .duration()
                .is_none_or(|duration| self.elapsed.saturating_add(1) < duration)
    }
    /// Phase, completed ticks within that phase, and phase duration (zero for open actions).
    pub fn phase(self) -> (AttackPhase, u32, u32) {
        if !self.is_running() {
            return (AttackPhase::Idle, 0, 0);
        }
        match self.schedule {
            ActionSchedule::Open => (AttackPhase::Active, self.elapsed, 0),
            ActionSchedule::Timed(t) if self.elapsed < t.startup => {
                (AttackPhase::Startup, self.elapsed, t.startup)
            }
            ActionSchedule::Timed(t) if self.elapsed < t.startup + t.active => {
                (AttackPhase::Active, self.elapsed - t.startup, t.active)
            }
            ActionSchedule::Timed(t) => (
                AttackPhase::Recovery,
                self.elapsed - t.startup - t.active,
                t.recovery,
            ),
        }
    }
    /// Report the captured schedule and current ownership without duplicating state.
    pub fn report(self, out: &mut arpg_core::Report) {
        let Self {
            schedule,
            controls,
            interruption,
            elapsed,
            outcome,
        } = self;
        out.int("elapsed", u64::from(elapsed));
        out.text(
            "outcome",
            match outcome {
                ActionOutcome::None => "none",
                ActionOutcome::Running => "running",
                ActionOutcome::Succeeded => "succeeded",
                ActionOutcome::Failed => "failed",
                ActionOutcome::Cancelled => "cancelled",
            },
        );
        out.text(
            "controls",
            match controls {
                ActionControls::None => "none",
                ActionControls::Movement => "movement",
                ActionControls::Facing => "facing",
                ActionControls::MovementAndFacing => "movement_and_facing",
            },
        );
        out.bool("committed", interruption == Interruption::Committed);
        match schedule {
            ActionSchedule::Open => out.text("schedule", "open"),
            ActionSchedule::Timed(ActionTimeline {
                startup,
                active,
                recovery,
            }) => {
                out.text("schedule", "timed");
                out.int("startup", u64::from(startup));
                out.int("active", u64::from(active));
                out.int("recovery", u64::from(recovery));
            }
        }
    }
    /// Every authoritative schedule, control and lifecycle field participates in replay.
    pub fn hash_into(self, hash: &mut Fnv) {
        let Self {
            schedule,
            controls,
            interruption,
            elapsed,
            outcome,
        } = self;
        schedule.hash_into(hash);
        hash.usize(controls as usize);
        hash.usize(interruption as usize);
        hash.u64(u64::from(elapsed));
        hash.usize(outcome as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_phases_retire_before_the_next_input_and_release_controls() {
        let mut execution = ActionExecution::new(
            ActionSchedule::timed(2, 2, 1).unwrap(),
            ActionControls::Facing,
            Interruption::Committed,
        );
        for (tick, expected) in [
            (AttackPhase::Startup, 0, 2),
            (AttackPhase::Startup, 1, 2),
            (AttackPhase::Active, 0, 2),
            (AttackPhase::Active, 1, 2),
            (AttackPhase::Recovery, 0, 1),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(execution.phase(), expected);
            assert!(execution.holds(ActionControls::Facing));
            assert!(!execution.interruptible());
            assert_eq!(execution.holds_next_tick(ActionControls::Facing), tick < 4);
            execution.advance();
        }
        assert_eq!(execution.outcome(), ActionOutcome::Succeeded);
        assert_eq!(execution.phase(), (AttackPhase::Idle, 0, 0));
        assert!(execution.interruptible());
        assert!(!execution.holds(ActionControls::Facing));
        execution.advance();
        assert_eq!(execution.elapsed(), 5);
    }
    #[test]
    fn execution_diagnostics_cover_lifetime_and_terminal_controls() {
        let hash = |execution: ActionExecution| {
            let mut hash = Fnv::default();
            execution.hash_into(&mut hash);
            hash.finish()
        };
        let mut execution = ActionExecution::new(
            ActionSchedule::Open,
            ActionControls::Movement,
            Interruption::Interruptible,
        );
        let initial = hash(execution);
        execution.advance();
        assert_ne!(hash(execution), initial);
        let running = hash(execution);
        execution.complete(false);
        assert_ne!(hash(execution), running);
        assert!(!execution.holds(ActionControls::Movement));
        let mut report = arpg_core::Report::default();
        execution.report(&mut report);
        let report = report.finish();
        for field in [
            "\"elapsed\":1",
            "\"outcome\":\"failed\"",
            "\"controls\":\"movement\"",
            "\"schedule\":\"open\"",
        ] {
            assert!(report.contains(field), "missing {field} in {report}");
        }
        let failure = execution;
        execution.cancel();
        execution.complete(true);
        execution.advance();
        assert_eq!(execution, failure, "terminal outcome is immutable");
    }
    #[test]
    fn open_and_wait_actions_use_the_same_results_and_control_rules() {
        let mut open = ActionExecution::new(
            ActionSchedule::Open,
            ActionControls::Movement,
            Interruption::Interruptible,
        );
        open.advance();
        assert_eq!(open.phase(), (AttackPhase::Active, 1, 0));
        assert!(open.holds_next_tick(ActionControls::Movement));
        open.cancel();
        open.advance();
        assert_eq!(open.outcome(), ActionOutcome::Cancelled);
        assert_eq!(open.elapsed(), 1);
        let mut wait = ActionExecution::new(
            ActionSchedule::timed(0, 1, 0).unwrap(),
            ActionControls::Movement,
            Interruption::Committed,
        );
        assert_eq!(wait.phase(), (AttackPhase::Active, 0, 1));
        wait.advance();
        assert_eq!(wait.outcome(), ActionOutcome::Succeeded);
        assert!(ActionSchedule::timed(0, 0, 0).is_err());
        assert!(ActionSchedule::timed(u32::MAX, 1, 0).is_err());
    }
}
