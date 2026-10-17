//! The engine capability independently of its future gameplay controller.

use arpg_core::Report;
use arpg_sim::{
    Accumulator, Condition, Dt, Source, SourceId, SourceSpec, SourceState, Template, World,
};

#[test]
fn setting_is_hashed_observable_idempotent_and_refuses_retired_names() {
    let mut world = World::empty();
    let id = world.add_source(
        Source::try_from(SourceSpec {
            pos: (20.0, 0.0),
            radius: 0.0,
            every: 1,
            when: Condition::Always,
            what: Template::BODY,
            enabled: true,
        })
        .unwrap(),
    );
    let enabled = world.hash();
    assert!(world.set_source_enabled(id, false));
    assert_ne!(enabled, world.hash());
    let paused = world.hash();
    let trace = world.trace().render();
    assert!(world.set_source_enabled(id, false));
    assert_eq!(world.hash(), paused);
    assert_eq!(world.trace().render(), trace);
    assert_eq!(
        world.source_state(id),
        Some(SourceState {
            enabled: false,
            countdown: 0,
            emitted: 0
        })
    );
    let mut out = Report::default();
    world.report(&mut out);
    assert!(
        out.finish().contains(
            "\"source_states\":{\"s0\":{\"enabled\":false,\"countdown\":0,\"emitted\":0}}"
        )
    );
    assert!(world.set_source_enabled(id, true));
    assert_eq!(
        world.hash(),
        enabled,
        "only the enablement bit changed, not the cadence"
    );
    assert!(world.remove_source(id));
    let next = world.add_source(
        Source::try_from(SourceSpec {
            pos: (30.0, 0.0),
            radius: 0.0,
            every: 1,
            when: Condition::Always,
            what: Template::BODY,
            enabled: true,
        })
        .unwrap(),
    );
    assert_ne!(id, next);
    let before = (world.hash(), world.trace().render());
    assert!(!world.set_source_enabled(id, true));
    assert!(!world.set_source_enabled(SourceId::parse("s999").unwrap(), false));
    assert_eq!(world.source_state(id), None);
    assert_eq!((world.hash(), world.trace().render()), before);
    for dt in Accumulator::default().pending(Dt::SECS) {
        let population = world.damageable_count();
        world
            .begin_step(dt, population)
            .advance(&[], &mut ())
            .finish();
    }
    assert_eq!(
        world.body_count(),
        1,
        "stale updates cannot disable the new source"
    );
}
