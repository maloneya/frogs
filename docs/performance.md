# Performance telemetry

## Accepted performance ceiling

Performance monitoring, benchmark cadence and the record-and-continue policy
apply on every branch. Performance results are observations, not task completion
gates. The user accepted the optimized renderer's current
performance as the new ceiling on 2026-10-17. The
[accepted measurement record](../benchmarks/evidence/accepted-20261017.md) owns
final release measurements, source identity, raw evidence and remaining uncertainty.
Use those workload-specific observations as the reference for subsequent changes,
with the suite's existing 10% relative tolerance, 15% stability rule and absolute
budgets. Do not replace this reference or widen gates without a user decision.

This acceptance preserves the current gameplay, authored assets, lighting,
shadows and weather. It does not certify every frame below the 60 Hz budget of
16.67 ms. Inconclusive measurements remain inconclusive; acceptance is a product
decision, not a timing pass. The [accepted-reference policy](../benchmarks/accepted-ceiling.json)
pins those immutable results for both direct runs and normal verification. Each
workload keeps its own source/binary provenance, including the historical dirty
source state; source changes do not prevent comparison. No later local or
published measurement replaces this ceiling automatically.

## Feature work: record and continue

Complete the assigned task when a performance regression, budget breach or
credible concern appears. Make a brief observation, retain the available evidence
and continue implementation and correctness checks. Performance alone is never
a reason to pause, abandon or revert the task, request permission to continue,
or withhold completion. Correctness failures still follow the verification rules.

Use the normal [benchmark cadence](verification.md), existing release workloads
and comparisons. For performance-sensitive work, identify the relevant workload
and retain any available pre-change evidence and source identity. There is no
extra per-increment measurement gate. If a baseline or foreground session is
unavailable, record the gap and continue; do not seize the desktop or start a
diagnostic measurement cycle to fill it.

Read individual workload/metric results: an overall `inconclusive` result can
contain a stable `regression`. Missing baselines do not erase absolute failures,
and an absolute breach alone does not prove the current task caused it. Debug
timings and invalid windows are concerns, not proof of release performance.

Before finishing, add or update a durable entry in
[performance follow-ups](performance-follow-ups.md) containing:

- The date, task/feature and changed files or commits that could account for the
  observation. Identify the measured revision and dirty source digest or snapshot;
  a commit name alone cannot identify dirty work. State when unrelated edits or
  missing pre-change evidence prevent attribution to this task.
- The workload/metric, observed trial values, baseline and limit when available.
  Label the entry as a measured regression, a suspected concern or missing coverage;
  separate known causes from hypotheses and unknowns.
- Links to retained raw evidence and source snapshots, plus the reproduction
  command, working directory and required conditions. Use existing artifacts;
  an unmeasured concern can name the relevant code and conditions instead.
- Completed and omitted coverage and the deferred question for later investigation.

Keep one entry per issue and append new task/source observations to it so later
work does not obscure when the regression first appeared. Preserve failing and
inconclusive results. Mention the entry and its practical impact briefly in the
final task report, even when the assigned work is complete. Recording a regression
does not accept it: the pinned reference, budgets and thresholds stay unchanged,
and a failed measurement never becomes a pass or advances a passing checkpoint.

Do not divert into profiling, research, ablations, optimization, asset reductions,
algorithm changes or repeated measurements to seek a pass. Do not automatically
start another agent or investigation. Such work needs the user's explicit scope;
a note does not expand the current task. When requested,
[investigate-performance](../.claude/skills/investigate-performance/SKILL.md)
uses the retained evidence to produce a decision brief. Implement only the
performance work the user authorizes.

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
| `acquire_ms` | Surface texture acquisition, including any drawable wait |
| `upload_ms` | Renderer staging, visibility, occupied-pose deformation, instance/palette packing and uploads |
| `encode_ms` | Render command recording |
| `submit_ms` | Command buffer finish and submission, including any backpressure |
| `present_ms` | Presentation notification and surface presentation |
| `capture_ms` | Optional synchronous screenshot readback and PNG write |
| `gpu_poll_ms` | Nonblocking progress/readback of earlier GPU observations; part of `other_ms` |
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
or GPU work. The renderer spans above are subsets of `render_cpu_ms`; `gpu_poll_ms`
is outside that call. Use headless benchmarks to isolate simulation cost from
presentation. Optional GPU observations are described below.

### Crowd rendering

[Character rendering](../crates/gfx/src/character.rs) classifies conservative
animated bounds separately against the main, directional and fire views. A
placement is uploaded once into a visibility-mask group; each pass submits only
its relevant groups. The soft grounding pass retains the union because a
footprint can be visible while its character is offscreen or airborne. This
adds bounded mask storage and sometimes draw ranges, rather than duplicating
all instance uploads. The transformed sphere is shared across view tests.

Horde deformation is performed once per occupied shared pose, then reused across
placements and render passes. The [posed geometry owner](../crates/gfx/src/character/posed.rs)
retains fixed-capacity vertex outputs and one-pose CPU staging. Every admitted
occupied pose is rewritten before use; removing the horde or replacing its asset
drops its resources. This is per-frame work reuse, with no pose approximation or
cross-frame invalidation policy. Geometry, influences, materials, proxy choice
and the existing pose budget are preserved. The single player stays GPU-skinned.
The tradeoff is CPU deformation and uploads proportional to occupied poses and
source vertices, plus bounded memory proportional to the admitted mesh. Measure
`upload_ms` alongside GPU savings; population alone does not bound a large asset's
preparation cost. This follows the established
[skin-cache mechanism](https://dev.epicgames.com/documentation/en-us/unreal-engine/skeletal-mesh-rendering-paths-in-unreal-engine),
using CPU preparation for this engine's already-shared crowd poses.

Lighting skips comparison sampling when its contribution is exactly zero,
without changing attenuation, filtering or light ranges. GPU reference tests
cover wet/dry shading, range boundaries and shadow states. Per-view GPU tests
compare union submissions, and posed-geometry tests compare the original
vertex-skinned player path; CPU/Metal rounding is reported separately from
geometry or animation changes.

Opaque static geometry and characters precede blended grounding, so Apple's
hidden-surface removal can reject the floor hidden by characters before shading
it. Character depth-pass samples clear a one-bit stencil mask; grounding draws
only on the remaining samples. That preserves the former late-character
overwrite even where feet intersect the grounding plane above the floor.
The main attachment uses explicit 32-bit depth plus stencil, with both discarded
after the pass; shadow maps remain depth-only. The extra attachment/tile storage
can offset the ordering benefit and needs measurement. GPU reference comparisons
include overlapping characters, contact heights, collapsed/airborne poses,
wet/dry lighting and close views. They require identical depth/stencil, unchanged
uncovered grounding pixels, and character pixels matching the opaque-only draw.
At benchmark resolution/distance, the full image comparison allows only sparse
one-code-value colour differences; close views expose larger isolated colour
differences from the former draw order. An HSR/quad numerical interaction is a
hypothesis, not an established cause. These are recorded separately from
geometry, contact-mask correctness and perceptual judgement. See
[Apple's opaque-first guidance](https://developer.apple.com/videos/play/wwdc2020/10602/).

The renderer rotates three complete camera/light/shadow binding sets. Rewriting
the single light/filter uniform or shadow map would otherwise depend on the
preceding frame's fragment reads. Separate sets let the next frame's shadow work
overlap those reads when the backend can schedule it. Queue-ordered writes still
protect reuse when the ring wraps; no CPU mapped-memory writes or completion
waits are introduced. Slots advance only after a submitted/presented frame.
The cost is two additional copies of the shadow maps (64 MiB with the current
owner's dimensions). Other shared resources may still constrain overlap; measure
redraw intervals and waits rather than adding per-pass GPU intervals. See
[Apple's resource-overlap example](https://developer.apple.com/videos/play/wwdc2021/10148/).

The configured maximum frame-latency hint is recorded in GPU context and must
match across compatible benchmark results. The renderer retains wgpu's default
hint of two (three Metal drawables). It is a configuration hint, not a
measurement of physical display latency. A lower hint can reduce queued-ahead
latency while hurting overlap, throughput and frame pacing; do not infer a
60 FPS improvement from that setting alone. See
[wgpu's presentation-queue contract](https://wgpu.rs/doc/wgpu/api/surface/type.SurfaceConfiguration.html#structfield.desired_maximum_frame_latency).

### GPU pass observations

Use `--gpu-timing` with the existing live runner, or launch with
`ARPG_GPU_TIMING=1`. Timing is off by default (`0`); other values are refused.
The renderer requests timestamp queries only for an enabled launch, checks adapter
support and reports requested/available state plus the timestamp period in `gpu`.

```sh
python3 scripts/perf.py run --mode live --gpu-timing --workload storm_fire --output target/perf/storm-gpu.json
```

The eight-slot readback ring never waits for the current submission. Full slots
leave that frame unsampled. Callbacks only signal completion; the frame loop polls
without waiting. Sampling completion must be observed before query resolution is
encoded and submitted: committing the resolve early can produce zero counters on
Metal, even in queue order ([upstream reproduction](https://github.com/gfx-rs/wgpu/issues/9414)).
Readback completes in a second asynchronous stage. `gpu_poll_ms` includes encoding
and submitting these old observations; it is outside `render_cpu_ms`. Fixed
query/resolve/readback buffers are reused after unmapping. The GPU test requires
all active counters in 128 real depth/color submissions, including full-ring
refusal and reuse. Separate pass query sets alone did not make live samples reliable.
The main render pass remains intact and still discards its depth attachment.

`gpu_directional_shadow_ms`, `gpu_fire_shadow_ms` and `gpu_main_ms` measure pass
boundaries. `gpu_span_ms` measures earliest pass start to latest end, including
gaps; it is neither a sum of pass costs nor end-to-end presentation latency.
Individual passes can overlap. The main pass includes geometry, grounding, effects,
weather and HUD; these observations do not isolate individual shaders or draws.

Every redraw has a monotonic `frame_id` that survives resets. Delayed GPU results
join that frame rather than the frame that received the callback. Reset discards
old rows/results and refuses previous-window completions. Summaries use only
results whose original frame remains in the retained window. `gpu_samples`,
`gpu_partial`, `gpu_pending`, `gpu_failed` and `gpu_unsampled` expose coverage; the last few
submitted frames can still be pending when a trial ends. Pass summaries include
their own sample counts. Raw rows omit unavailable GPU durations and report
`gpu_status`; absent passes never become zero-cost samples. Partial readbacks retain
valid individual pass times and omit the full span. `gpu_unavailable_passes` is a
bit mask for directional/fire/main passes (bits 0/1/2). Missing/discontinuous pairs
set their bit and omit their duration; zero and Metal's all-ones error sentinel are
invalid. Map/readback failures invalidate diagnostic
evidence. Per-pass coverage must accompany any attribution, since missing samples
can bias the available subset. Empty Metal passes can omit an end
timestamp, so even an advertised feature does not guarantee every observation.

Telemetry schema 2 and benchmark evidence schema 6 include this instrumentation
context. Instrumented/ordinary live runs cannot be compared as equivalent settings.
Run the same workload both ways to estimate GPU instrumentation overhead, including
noise. CPU clock reads remain in both modes; this comparison does not establish
their overhead. Timings never feed simulation, replay or hash state.

`perf`/`perf samples` share these fields. Their `latest` object and the companion's
Debug readout show the last completed CPU frame and latest available GPU frame
with separate IDs. Companion polling is still excluded from benchmark sessions.

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

Validate each trial before collecting another. An invalid trial stops collection
and preserves raw evidence; a measured regression does not skip the other selected
workloads. Unfinished workloads/repetitions remain inconclusive. A stable failing
metric is prominently reported even when aggregate coverage is inconclusive. No
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
settings. Current evidence records benchmark scene/driver identity separately
from every live `.glb` asset identity. The [runner](../scripts/perf.py) admits only
its current evidence schema; capture fresh evidence rather than relabeling old
artifacts.

For a user-triggered asset investigation, measure the same focused workload in
isolated before/after source states, then declare every changed asset explicitly:

```sh
python3 scripts/perf.py compare target/perf/before.json target/perf/after.json \
  --asset-change assets/characters/wolf/wolf.glb \
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

Without `--baseline`, the runner loads the pinned accepted references for the
requested mode. A full suite can use separately captured workload results; no
synthetic measurement combines their source or binary identities. Every pinned
artifact is checked against its content address, manifest and raw measurements
before building. Missing references, corrupt artifacts, changed workload policy
or incompatible measurement conditions refuse collection. Missing relative
coverage cannot become an absolute-only pass.

Use `--baseline RESULT.json` for an explicit investigation comparison. A dirty
measurement retains its original source identity even after those edits are
committed. Publishing or collecting newer evidence does not change the accepted
reference; that requires a user decision. Evidence storage is excluded from
source fingerprints, while the accepted-reference policy is a source input.
Failed/incomplete runs remain at their requested path. Collected trials survive
assessment/archive failures and can be compared again.

### Interpreting the result

| Exit | Status | Meaning |
|---|---|---|
| 0 | `pass` | Absolute budgets and every required relative comparison passed |
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
passing while relative coverage and the overall result remain inconclusive. A noisy metric does not
erase healthy metrics; invalid raw evidence invalidates its workload.

All repetitions must be below an absolute budget to pass, or above it to regress.
A relative pass requires the candidate's slowest trial within tolerance of the
baseline's fastest; regression requires the candidate's fastest beyond tolerance
of the baseline's slowest. Overlap or excessive spread is inconclusive. Rerun
uncertain workloads under stable conditions, preserving previous samples, within
a user-triggered investigation. Feature agents record the observation and continue
their assigned work without diagnostic retries.

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
   unconfirmed beyond available pass observations and include the missing capture
   in the decision brief. On macOS 27, `metalperftrace collect --last 5m <directory>`
   can recover system Metal metrics without Instruments. Enable detailed signposts
   on the owned process before measurement with
   `metalperftrace setup --enable per-frame-metrics --pid <pid>` and disable them
   afterwards. This can substantially increase trace size. Export with
   `metalperftrace overview --json --json-include-timeline --predicate 'processName == "arpg"' <trace>`.
   Correlate PID/time boundaries with the controlled trial; unlabelled lookbacks
   cannot establish scene/source identity. Aggregate windows cannot reconstruct
   frame percentiles when per-frame rows are absent. On the tested macOS 27 CLI,
   `--json-include-timeline` still exported one-second `Stats Timeline` aggregates
   after enabling those signposts. Retain the trace for Instruments; do not claim
   per-frame on-glass coverage from that JSON. Trial request/reply UTC brackets
   can select conservative interior aggregate bins, but cannot join individual
   Metal frames to app frame IDs. Extra profiling submissions can also change
   system command-buffer/frame association: use an ordinary-run control for
   end-to-end presentation latency, and report that observer effect.
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
