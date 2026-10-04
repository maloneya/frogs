//! Typed, scoped transition predictions. No diagnostic identity can be used as
//! a live engine handle. Matching never depends on event formatting or retention.

use arpg_sim::{Event, EventKind};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Events {
    pub(crate) kinds: Vec<EventKind>,
    pub(crate) sequence: Vec<(u64, ExpectedEvent)>,
}

/// Typed transition payloads. Identities are read-only diagnostic values.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum ExpectedEvent {
    TuningChanged {
        field: arpg_sim::TuningField,
        #[serde(deserialize_with = "crate::spec::tolerance")]
        value: f32,
    },
    SceneLoaded { id: String },
    SceneEvicted { id: String },
    Impulsed {
        id: (u32, u32), source: Option<(u32, u32)>,
        #[serde(deserialize_with = "crate::spec::finite_pair")]
        value: (f32, f32),
        #[serde(deserialize_with = "crate::spec::tolerance")]
        tolerance: f32,
    },
    Activated { id: (u32, u32) },
    Placed { id: (u32, u32) },
    Removed { id: (u32, u32) },
    LatchBroken { id: (u32, u32) },
    SourceAdded { id: String },
    SourceRemoved { id: String },
    SourceEnablementChanged { id: String, enabled: bool },
    Fired { source: String },
    Refused { count: usize },
    Momentum { pairs: usize },
    Rested { count: usize },
    Contacts { count: usize },
    Crowded { count: usize },
    WallContacts { count: usize },
    Clamped { count: usize },
    Swung,
    ChannelStarted,
    ChannelStopped,
    HitboxOpened,
    HitboxClosed,
    Damaged { actor: (u32, u32), target: (u32, u32), remaining: u8 },
    Hit { id: (u32, u32), remaining: u8 },
    AttackRecoveryChanged { recovery: arpg_sim::RecoveryTicks },
    AttackProfileChanged { profile: arpg_sim::AttackProfile },
}

impl ExpectedEvent {
    pub(crate) fn kind(&self) -> EventKind {
        match self {
            Self::TuningChanged { .. } => EventKind::TuningChanged,
            Self::SceneLoaded { .. } => EventKind::SceneLoaded,
            Self::SceneEvicted { .. } => EventKind::SceneEvicted,
            Self::Impulsed { .. } => EventKind::Impulsed,
            Self::Activated { .. } => EventKind::Activated,
            Self::Placed { .. } => EventKind::Placed,
            Self::Removed { .. } => EventKind::Removed,
            Self::LatchBroken { .. } => EventKind::LatchBroken,
            Self::SourceAdded { .. } => EventKind::SourceAdded,
            Self::SourceRemoved { .. } => EventKind::SourceRemoved,
            Self::SourceEnablementChanged { .. } => EventKind::SourceEnablementChanged,
            Self::Fired { .. } => EventKind::Fired,
            Self::Refused { .. } => EventKind::Refused,
            Self::Momentum { .. } => EventKind::Momentum,
            Self::Rested { .. } => EventKind::Rested,
            Self::Contacts { .. } => EventKind::Contacts,
            Self::Crowded { .. } => EventKind::Crowded,
            Self::WallContacts { .. } => EventKind::WallContacts,
            Self::Clamped { .. } => EventKind::Clamped,
            Self::Swung => EventKind::Swung,
            Self::ChannelStarted => EventKind::ChannelStarted,
            Self::ChannelStopped => EventKind::ChannelStopped,
            Self::HitboxOpened => EventKind::HitboxOpened,
            Self::HitboxClosed => EventKind::HitboxClosed,
            Self::Damaged { .. } => EventKind::Damaged,
            Self::Hit { .. } => EventKind::Hit,
            Self::AttackRecoveryChanged { .. } => EventKind::AttackRecoveryChanged,
            Self::AttackProfileChanged { .. } => EventKind::AttackProfileChanged,
        }
    }

    pub(crate) fn matches(&self, actual: Event) -> bool {
        match (self, actual) {
            (Self::TuningChanged { field, value }, Event::TuningChanged { field: actual, value: changed }) =>
                *field == actual && *value == changed,
            (Self::SceneLoaded { id }, Event::SceneLoaded { id: actual })
            | (Self::SceneEvicted { id }, Event::SceneEvicted { id: actual }) => arpg_sim::SceneId::parse(id) == Some(actual),
            (Self::Impulsed { id, source, value, tolerance }, Event::Impulsed { id: actual, source: origin, impulse }) => {
                let (x, z) = impulse.components();
                *id == actual.diagnostic_identity() && *source == origin.map(arpg_sim::EntityId::diagnostic_identity)
                    && tolerance.is_finite() && *tolerance >= 0.0
                    && (value.0 - x).abs() <= *tolerance && (value.1 - z).abs() <= *tolerance
            }
            (Self::Activated { id }, Event::Activated { id: actual })
            | (Self::Placed { id }, Event::Placed { id: actual })
            | (Self::Removed { id }, Event::Removed { id: actual })
            | (Self::LatchBroken { id }, Event::LatchBroken { id: actual }) => *id == actual.diagnostic_identity(),
            (Self::Damaged { actor, target, remaining }, Event::Damaged { actor: a, target: t, remaining: r }) => *actor == a.diagnostic_identity() && *target == t.diagnostic_identity() && *remaining == r,
            (Self::Hit { id, remaining }, Event::Hit { id: actual, remaining: value }) => *id == actual.diagnostic_identity() && *remaining == value,
            (Self::SourceAdded { id }, Event::SourceAdded { id: actual })
            | (Self::SourceRemoved { id }, Event::SourceRemoved { id: actual }) => arpg_sim::SourceId::parse(id) == Some(actual),
            (Self::SourceEnablementChanged { id, enabled }, Event::SourceEnablementChanged { id: actual, enabled: value }) => arpg_sim::SourceId::parse(id) == Some(actual) && *enabled == value,
            (Self::Fired { source }, Event::Fired { source: actual }) => arpg_sim::SourceId::parse(source) == Some(actual),
            (Self::Refused { count }, Event::Refused { count: actual })
            | (Self::Momentum { pairs: count }, Event::Momentum { pairs: actual })
            | (Self::Rested { count }, Event::Rested { count: actual })
            | (Self::Contacts { count }, Event::Contacts { count: actual })
            | (Self::Crowded { count }, Event::Crowded { count: actual })
            | (Self::WallContacts { count }, Event::WallContacts { count: actual })
            | (Self::Clamped { count }, Event::Clamped { count: actual }) => *count == actual,
            (Self::AttackRecoveryChanged { recovery }, Event::AttackRecoveryChanged { recovery: actual }) => *recovery == actual,
            (Self::AttackProfileChanged { profile }, Event::AttackProfileChanged { profile: actual }) => *profile == actual,
            (Self::Swung, Event::Swung) | (Self::ChannelStarted, Event::ChannelStarted)
            | (Self::ChannelStopped, Event::ChannelStopped) | (Self::HitboxOpened, Event::HitboxOpened)
            | (Self::HitboxClosed, Event::HitboxClosed) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{run, spec::Scenario};
    use std::path::Path;

    fn scenario(sequence: &str, kinds: &str) -> Scenario {
        ron::from_str(&format!("#![enable(implicit_some)] (attacks: [0], budget: (ticks: 14), expect: (events: (kinds: [{kinds}], sequence: [{sequence}])) )")).unwrap()
    }

    #[test]
    fn exact_scope_rejects_missing_extra_early_and_wrong_payload_events_even_when_blessing() {
        let valid = scenario("(0, Swung), (6, HitboxOpened), (12, HitboxClosed)", "Swung, HitboxOpened, HitboxClosed, Hit");
        assert!(run::validate(&valid).is_empty());
        let observed = run::run(&valid);
        assert!(run::check_trace(&valid, &observed, Path::new("unused"), false).is_none());
        for sequence in [
            "(0, Swung), (5, HitboxOpened), (12, HitboxClosed)",
            "(0, Swung), (6, HitboxOpened)",
            "(0, Swung), (6, HitboxOpened), (12, HitboxClosed), (13, HitboxClosed)",
            "(0, Swung), (6, HitboxOpened), (12, Hit(id: (1, 1), remaining: 2))",
        ] {
            let wrong = scenario(sequence, "Swung, HitboxOpened, HitboxClosed, Hit");
            assert!(run::check_trace(&wrong, &observed, Path::new("unused"), true).is_some());
        }
    }

    #[test]
    fn oversized_expectations_are_rejected_before_capture() {
        let mut oversized = scenario("", "Swung");
        oversized.expect.events.as_mut().unwrap().sequence.resize_with(arpg_sim::MAX_TRACE_EVENTS, || (0, ExpectedEvent::Swung));
        assert!(!run::validate(&oversized).is_empty());
    }


    #[test]
    fn count_payloads_are_checked_independently_of_event_category() {
        let expected = ExpectedEvent::Refused { count: 2 };
        assert!(expected.matches(Event::Refused { count: 2 }));
        assert!(!expected.matches(Event::Refused { count: 1 }));
        assert!(!expected.matches(Event::Contacts { count: 2 }));
    }

    #[test]
    fn empty_scope_and_predictions_outside_scope_or_budget_fail_validation() {
        for (sequence, kinds) in [("", ""), ("(0, Swung)", "Hit"), ("(14, Swung)", "Swung")] {
            assert!(!run::validate(&scenario(sequence, kinds)).is_empty());
        }
        assert!(ron::from_str::<ExpectedEvent>("Swung(count: 1)").is_err());
        assert!(ron::from_str::<ExpectedEvent>("Hit(id: (1, 1), remaining: 2, count: 1)").is_err());
    }
}
