# Source enablement

The engine owns whether a source can advance. Gameplay can request a change
without owning a second enabled flag or reaching body storage. The
[source-control mechanic](source-control.md) uses this boundary.

Sources start enabled and ready unless authored with `enabled: false`.
Disabling freezes both the remaining countdown and the emission count used for
ring placement. Enabling resumes them. A ready source may emit on the next
source evaluation, provided its existing population or proximity condition is
met. A source waiting on a condition remains ready.

For example, a source with cadence four fires at tick 2 and ends that tick with
countdown three. Tick 3 reduces it to two. Disabling before tick 4 and enabling
before tick 8 preserves that two-tick wait: ticks 8 and 9 consume it, and tick 10
fires. There is no reset or catch-up burst.

`World::set_source_enabled` and the corresponding `Game` operation apply
immediately between ticks. They return true for a live source, including an
unchanged setting, and false for an absent or retired identity. Only an actual
transition produces `Event::SourceEnablementChanged`. The setter cannot access
the spawn queue: requests already accepted still land. Eviction retains its
separate responsibility to cancel work owned by the evicted scene.

`World::source_state` and `Game::source_state` return a copy of `SourceState`:
enabled, countdown, and accepted emission count. The same stored type supplies
the state report and scenario assertions. Reports expose a `source_states` object
keyed by source ID; the existing `sources` field remains the live count.
All three state fields participate in the deterministic hash.

Sources in additive scene instances retain independent state. Eviction retires
their IDs for the current run. Restart restores authored enablement, a ready
countdown, and zero emissions from the cached scene. IDs are scoped to a run.

## Driving and asserting

The harness accepts:

```text
source 20 0 every 4 ring 2 disabled
source enable s0
source disable s0
state
```

The optional disabled flag must precede a template, because template parsing
consumes the rest of the command. Scene and scenario sources use the same
`SourceSpec` field, `enabled: false`.

Scenario switches run before removals and source evaluation on their named
zero-based tick. Both switches and state assertions index `setup.sources`,
starting at zero. An optional `scene` load index instead selects that scene's
original source bindings. Removing an earlier source cannot renumber later targets. An unknown index,
unreachable switch tick, or switch targeting a removed source fails the run.

```ron
setup: (sources: [(pos: (20.0, 0.0), every: 4, enabled: false)]),
source_switches: [(at: 2, source: 0, enabled: true)],
checkpoints: [
    (at: 1, expect: (source_states: [
        (source: 0, state: Some((enabled: false, countdown: 0, emitted: 0)))
    ])),
    (at: 2, expect: (source_states: [
        (source: 0, state: Some((enabled: true, countdown: 3, emitted: 1)))
    ])),
],
```

Use `state: None` to assert removal of a previously installed setup source.
The cadence scenario pins transition and emission ticks in its golden trace;
the population and proximity scenarios verify that enabling still respects
those conditions. These all run through `Game::step` and replay checks.
