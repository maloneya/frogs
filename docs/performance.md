# Performance telemetry

## Feature work: stop and handoff

Before changing a feature likely to affect crowd cost, animation, assets or
rendering, identify the representative workload and retain compatible release
evidence for the pre-feature state. Measure a small coherent increment before
stacking further expensive changes; benchmark cadence is not a reason to defer
that check. Use existing gates and workloads. If live conditions or a compatible
baseline are unavailable, flag the missing coverage before proceeding with
performance-sensitive work.

At the first stable absolute-budget breach, relative regression or credible
performance concern, immediately stop feature work and notify the user. Read
individual workload/metric results: an overall `inconclusive` result can contain
a stable `regression`. Missing baselines do not erase absolute failures, and an
absolute breach alone does not prove the new feature caused it. A debug timing
failure or invalid window is a concern to flag, not proof of release performance.

Preserve the candidate, source identity, raw trials and previous evidence. Report
only what is already available; do not spend another measurement cycle proving
the alert. The short handoff gives:

- The feature and last coherent change being implemented, and current state.
- The affected workload and metric, measured trial values, limit and compatible
  before-change values if available; distinguish failure from uncertainty.
- Evidence paths, source/snapshot identity and the reproduction command with
  its working directory and required conditions.
- What is known about cause, what remains unmeasured, and completed/omitted checks.

Do not tune, cull, approximate, simplify assets, change algorithms, loosen gates,
replace baselines or keep rerunning until green. Do not launch an investigation
or another agent automatically. A feature stopped here is unfinished; preserve
the work without reverting it or claiming completion. Reading the existing
report and safely ending an owned measurement session are sufficient wrap-up.

The user triggers [investigate-performance](../.claude/skills/investigate-performance/SKILL.md)
in this chat or another one. That workflow separates measurement, attribution
and published alternatives from implementation, then returns a decision brief.
Only the user's selected approach authorizes a fix or a change in feature scope.

## Measuring

Use a release build with `ARPG_HARNESS`, a visible frontmost window and an awake
display. Follow the [live playtest guide](../.claude/skills/playtest/SKILL.md).
Warm up the scene, then collect before exporting or capturing:

```sh
echo 'perf reset' | nc -U /tmp/arpg.sock
echo 'wait 5000' | nc -U /tmp/arpg.sock
echo 'perf' | nc -U /tmp/arpg.sock
echo 'perf samples' | nc -U /tmp/arpg.sock > /tmp/arpg-perf.json
```

`perf` reports the retained frame window without hashing Game or serializing
bodies. `perf samples` adds chronological frame rows. Check `samples`, `window_ms`
and `overwritten`: the bounded window holds less time when running uncapped.
Reporting and capture can perturb the following frame; reset before a new trial.

## Timing boundaries

A frame interval runs from one redraw start to the next, including work and
waiting. The first redraw after reset establishes a baseline; the newest frame
remains pending until the next start.

| Field | Meaning |
|---|---|
| `frame_ms` | Redraw-start interval |
| `simulation_ms` | Input preparation, tick loop and per-tick effect observations |
| `game_step_ms` | Game::step calls, summed per frame; part of simulation_ms |
| `crowd_ms` | Grid rebuilding and contact resolution for the tick; part of game_step_ms |
| `presentation_ms` | Camera, animation, geometry preparation and HUD |
| `render_cpu_ms` | Surface acquisition, uploads, submission, present and capture |
| `other_ms` | Remaining interval, including event handling and waits |

Summaries provide mean, nearest-rank p50/p95/p99 and maximum in milliseconds.
`mean_step_ms` and `mean_crowd_step_ms` divide by executed ticks, distinguishing
expensive ticks from catch-up. Check tick counts before interpreting zero means.
`max_ticks_per_frame`, `zero_tick_frames` and `multi_tick_frames` expose pacing.

`crowd_candidates` and `crowd_possible_pairs` sum exact tests and potential
non-player pairs for each tick's first collision pass in retained ticks,
including props. Their ratio measures spatial filtering. Raw-frame contact counts
instead describe the last completed tick, even on a zero-tick frame.

`presented_fps` and `ticks_per_second` use the same window duration.
`surface_skipped` counts redraws without a presented surface, not missed display
deadlines. `over_budget` uses a fixed 60 Hz target; `over_1_5_budget` counts
intervals over 25 ms. Neither proves physical display delivery.

**Renderer CPU time is not GPU time.** It can include waits for vsync, a surface
or GPU work. GPU execution is unmeasured. Use headless benchmarks to isolate
simulation cost from presentation.

## Reset and lifetime

`perf reset` clears measurements without changing Game or pending inputs.
Successful scene start/restart/load/eviction, character/horde replacement,
vsync changes and resize also reset it. Rejections, ordinary spawns and combat
do not. After changing zoom, debug drawing or other conditions, reset explicitly.

## Repeatable benchmark suite

[Verification](verification.md) runs due headless benchmarks. Live measurement
requires an uninterrupted foreground session and an explicit request:

```sh
python3 scripts/verify.py --only performance-live
```

The direct runner offers focused measurement and comparison:

```sh
python3 scripts/perf.py run --mode headless --output target/perf/candidate.json
python3 scripts/perf.py run --mode live --output target/perf/live.json
python3 scripts/perf.py archive target/perf/candidate.json
python3 scripts/perf.py compare baseline.json candidate.json --output comparison.json
python3 scripts/perf.py run --mode headless --rerun candidate.json --output retry.json
```

Choose fresh output paths. Live mode builds release, owns its game/private
harness, and refuses competing `arpg` processes. Keep the window visible and
leave input alone; close the companion to avoid polling/rendering overhead.

[The suite](../benchmarks/suite.json) owns workloads, warm-up/sample lengths,
repetitions, budgets and tolerances. Use repeatable `--workload` flags to focus.
`--vsync off` supports throughput investigations but can exhaust the recorder;
changing sample windows requires a suite edit shared by both compared runs.
The [trial driver](../benchmarks/driver.rs) owns exact-tick gameplay inputs.
Each repetition restarts its scene. Hashing and serialization are outside timing.
Boundary frames discard excess catch-up ticks; interior frames use the normal
accumulator. Native input or resizing invalidates a live trial.

The runner requests foreground activation once through `perf prepare`, then
waits at most five seconds for current readiness and three recent presenting,
focused, unoccluded frames. Each trial is admitted before scene restart only
when the window is focused, unoccluded and has a nonzero surface. Losing focus
or becoming occluded during warm-up also invalidates the trial. Readiness
observation never repeatedly activates the app. A reserved foreground session
is still required; agents do not use ad hoc desktop activation scripts.

Validate each trial before collecting another. At the first invalid trial or
completed workload needing review, collection stops and preserves raw evidence.
Unfinished workloads/repetitions remain inconclusive. A stable failing metric
is prominently reported even when aggregate coverage is inconclusive. No
automatic retries occur, and `--rerun` refuses evidence containing a confirmed
failure; the user's investigation can explicitly select a workload instead.

`night_fire` and `storm_fire` are a benchmark-owned matched pair: 10,000 actors,
one fire and four tree blockers. The second enables rain, wet highlights and
lightning. Their 900 warm-up and 240 sample ticks cover the next lightning
strokes using ordinary tick-derived presentation time. Select both for weather
attribution; use headless evidence to isolate simulation. Their different
authored environment hashes intentionally prevent treating one as the other's
ordinary baseline. They retain the suite's existing step/frame limits.

### Comparing intentional asset changes

Ordinary comparisons still require identical asset fingerprints and presentation
settings. Schema 3 records benchmark scene/driver identity separately from every
live `.glb` asset identity; previous schemas cannot reconstruct that separation
and are excluded. Capture fresh evidence rather than relabeling old artifacts.

For a user-triggered asset investigation, measure the same focused workload in
isolated before/after source states, then declare every changed asset explicitly:

```sh
python3 scripts/perf.py compare target/perf/before.json target/perf/after.json \
  --asset-change assets/characters/basic-enemy/basic-enemy.glb \
  --output target/perf/asset-comparison.json
```

This permits changed mesh/texture/rig counts only for the declared character
asset roles. All other assets, asset activation, machine/compiler conditions,
workload policy, camera, present mode and gameplay/population outcomes must
match. Missing, extra or duplicate declarations refuse relative comparison.
Absolute budgets remain applicable. The result records asset hashes, candidate
source/binary and baseline provenance; a feature comparison is not eligible
baseline evidence and cannot be archived as a measurement.

Only asset differences and measurement conditions are checked automatically.
Source fingerprints identify both builds but do not prove absence of unrelated
code edits. Retain exact snapshots and disclose any such edits before making a
causal claim. This path does not authorize an optimization or accept new budgets.

### Evidence and baselines

Structurally valid runs, including noisy ones, automatically enter ignored
`target/perf/evidence`. `archive` publishes immutable compressed evidence and
provenance to [benchmarks/evidence](../benchmarks/evidence/README.md) for other
checkouts. Commit both files. There is no automatic baseline replacement or bless.

Without `--baseline`, the runner selects the nearest compatible ancestor from
both stores before measuring. It validates raw artifacts, not just manifest
verdicts. Corrupt eligible evidence fails preflight; incompatible schemas are
excluded. Dirty evidence is reusable automatically only for an identical source
digest; otherwise select it explicitly. A dirty measurement does not become clean
evidence merely because its source is committed later.

To seed reusable evidence, measure a clean commit, then archive and commit the
artifacts separately. Their provenance identifies the measured source, not the
later evidence commit. Evidence storage is excluded from source fingerprints.
Failed/incomplete runs remain at their requested path without becoming baselines.
Collected trials survive assessment/archive failures and can be compared again.

### Interpreting the result

| Exit | Status | Meaning |
|---|---|---|
| 0 | `pass` | Applicable budgets and comparisons passed |
| 1 | `regression` | Stable evidence exceeded an absolute or relative limit |
| 2 | `inconclusive` | Missing, invalid, incompatible, unstable or borderline evidence |

Results retain source, binary, workload, asset, machine and build fingerprints,
raw timings and gameplay outcomes. Source/binary changes invalidate measurement.
Comparisons require compatible conditions and repeatable outcomes; hash equality
is scoped to this machine, not cross-platform replay.

Fixed-population workloads must retain every enemy. Combat trials must defeat
bodies and match baseline population observations; they do not establish
constant-population combat capacity. Live evidence also requires complete frame
coverage without skipped, overwritten, unfocused or occluded frames.

Relative gates compare mean Game step cost, mean presentation preparation and
p99 frame interval. Renderer CPU time is diagnostic: less simulation work may
increase time waiting inside the renderer. Headless mode measures only mean
Game::step cost and cannot establish rendering capacity.

Inspect `absolute_status`, `relative_status`, individual metrics and
`rerun_workloads`. Missing/incompatible baselines can leave absolute budgets
passing while relative coverage remains inconclusive. A noisy metric does not
erase healthy metrics; invalid raw evidence invalidates its workload.

All repetitions must be below an absolute budget to pass, or above it to regress.
A relative pass requires the candidate's slowest trial within tolerance of the
baseline's fastest; regression requires the candidate's fastest beyond tolerance
of the baseline's slowest. Overlap or excessive spread is inconclusive. Rerun
uncertain workloads under stable conditions, preserving previous samples, within
a user-triggered investigation. Feature agents follow the stop rule above.

## Agent maintenance contract

Keep bounded coarse telemetry and representative workloads; add detailed probes
only for a concrete investigation and measure their overhead. Performance claims
must include workload, before/after evidence, uncertainty and gameplay verification.
Run the full suite before claiming engine-wide capacity improvement. Intentional
behaviour/workload changes need reviewed baseline evidence, not relaxed gates.

Scenario budgets are correctness smoke checks, not substitutes for live frame
pacing or human judgement of combat feel. Evidence-policy tests run with the
workspace suite or `python3 -B -m unittest discover -s scripts -p test_perf.py`.

## GPU investigation

Start from retained frame rows and a stated hypothesis: GPU execution, CPU
submission, presentation waits or simulation catch-up. `render_cpu_ms` cannot
distinguish these. Use a separate diagnostic capture of the same release binary,
benchmark-owned scene, camera, inputs and presentation mode; never treat the
instrumented capture as ordinary benchmark evidence.

1. Check tool availability with `xcrun --find xctrace` and
   `xcrun xctrace list templates`. Full Xcode/Instruments is required; command-line
   tools alone may not provide them. If unavailable, record GPU attribution as
   unconfirmed and include the missing capture in the decision brief.
2. Launch through the [harness](../.claude/skills/playtest/SKILL.md) with private
   sockets and a reserved foreground session. Load the selected benchmark scene
   and use its exact `perf trial` schedule. Record source/binary/asset identities,
   workload, window dimensions, present mode, GPU and capture conditions.
3. In Instruments, choose Game Performance or Metal System Trace and target the
   owned game process. Record warm-up and measurement. Inspect CPU/GPU overlap,
   submission gaps, waits and GPU activity around slow frame intervals. Save the
   `.trace` and identify the relevant time range; don't infer GPU duration from
   a CPU span. Follow [Apple's performance analysis guide](https://developer.apple.com/documentation/xcode/analyzing-the-performance-of-your-metal-app).
4. If the timeline points to GPU work, capture a relevant Metal workload in
   Xcode, enable profiling of the GPU trace, and inspect pass durations and
   supported counters. Distinguish shadow generation, visible geometry and
   fragment/filtering cost using the renderer's actual passes. Save the GPU
   trace with performance data. Follow [Apple's capture guide](https://developer.apple.com/documentation/xcode/capturing-a-metal-workload-in-xcode)
   and [workload analysis](https://developer.apple.com/documentation/xcode/analyzing-your-metal-workload).

Compare a controlled feature ablation only if that evidence would discriminate
the remaining options. Report measured durations, counters, instrument overhead,
source differences and uncertainty. A single captured frame may explain work
without establishing p99 pacing or capacity. Return to the user's decision;
ordinary uninstrumented benchmarks validate an implementation only after selection.

## Detailed physics investigation

The opt-in headless profiler runs the ordinary `Game::step_observed` schedule.
Clocks remain in the executable; the simulation supplies read-only phase changes
and work counts. The ordinary benchmark and live recorder keep their existing
coarse measurements. Use a benchmark-owned scene and a fresh artifact path:

```sh
cargo build --release -p scenario --bin perf-headless
target/release/perf-headless benchmarks/compressed.ron 900 180 idle --profile > target/physics-profile.json
target/release/perf-headless benchmarks/compressed.ron 900 180 idle > target/physics-plain.json
python3 scripts/physics_profile.py target/physics-profile.json --plain target/physics-plain.json
```

`physics_profile.ticks` uses completed simulation tick numbers. `phase_ns` contains
exclusive durations: phases sum to the crowd pass's duration, not the entire
Game step. The remaining step time includes gameplay and instrumentation outside
those boundaries. `totals` sums durations and work over sampled ticks. Warm-up is uninstrumented and unreported.
No reports, hashing or profile-buffer growth occur inside the measured step.

[The observer](../crates/sim/src/observe.rs) owns phase and counter definitions,
following the [soft step](crowd-collision.md#soft-step): broadphase, crowd and
solid contact tests, preparation, then per-substep warm start, solve, integrate
and relax, the continuous sweep, and storing warm-start forces. Counters report
collision passes, substeps, body and solid rows, speculative rows, swept bodies
and lock breaks, summed over every collision pass in the tick; candidates count
the first pass only. More
than one collision pass means something moved fast enough to split the tick.

Compare identical instrumented/plain schedules and hashes before interpreting
instrumentation overhead. Alternate repeated trials on an otherwise idle machine;
a single difference includes noise. These diagnostic artifacts do not replace
suite budgets, compatible baselines, or live rendering evidence. Profiling does
not change physics inputs or gameplay decisions.
