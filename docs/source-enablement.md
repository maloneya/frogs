# Source enablement

The engine owns whether a source can advance. Gameplay can request a change
without owning a second enabled flag or reaching body storage. The
[encounter mechanic](encounters.md) uses this boundary.

Sources start enabled and ready unless authored with `enabled: false`.
Disabling freezes both the remaining countdown and the emission count used for
ring placement. Enabling resumes them. A ready source may emit on the next
source evaluation, provided its existing population or proximity condition is
met. A source waiting on a condition remains ready.

`FewerThan(n)` tests the world's hostile actor count before births are drained.
Every source evaluated in that tick sees the same count, so simultaneous emissions
can exceed `n`; it is a refill threshold, not a hard population cap. Health,
movement and presentation do not determine that count.
[Source triggering](../crates/sim/src/pass/source.rs) owns cadence and gate evaluation;
[Game](../crates/game/src/population.rs) supplies actor definitions and population.

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

`Game::source_state` returns enabled, countdown and emission state. Reports expose
`source_states` keyed by source ID; `sources` is the live count.

Sources in additive scene instances retain independent state. Eviction retires
their IDs for the current run. Restart restores authored enablement, a ready
countdown, and zero emissions from the cached scene. IDs are scoped to a run.

## Driving and asserting

The harness accepts:

```text
source (pos: (20.0, 0.0), every: 4, radius: 2.0, enabled: false)
source enable s0
source disable s0
state
```

Harness, scene and scenario sources use the same game-owned `SourceSpec` description,
including `enabled: false`.

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
