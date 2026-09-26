# Performance telemetry

The app records the latest 1,024 completed redraw intervals in a fixed-capacity
buffer. Collection uses monotonic clocks and does not allocate per frame.
Telemetry belongs to presentation: Game never reads it, and it does not enter
simulation state, hashes or replay. Percentiles and JSON are built only on request.

## Measuring

Use a release build with `ARPG_HARNESS`, a visible frontmost window and an awake
display. Follow the [live playtest guide](../.claude/skills/playtest/SKILL.md).
Warm up the scene before resetting the window:

```sh
echo 'perf reset' | nc -U /tmp/arpg.sock
echo 'wait 5000' | nc -U /tmp/arpg.sock
echo 'perf' | nc -U /tmp/arpg.sock
# Optional bounded export of individual frames:
echo 'perf samples' | nc -U /tmp/arpg.sock > /tmp/arpg-perf.json
```

`perf` does not hash Game or serialize bodies. It includes the current run id,
tick, enemy count and vsync setting plus the retained-window summary.
`perf samples` adds `frames`, keyed from zero in chronological order. Each row
has the same timing vocabulary plus simulation ticks executed, ending tick,
enemy count, last completed tick's crowd-contact count and whether a frame was
presented. Contact counts are not summed over catch-up ticks; on a zero-tick
frame they still describe the previous tick.

The window holds approximately 17 seconds at 60 FPS, but less time uncapped.
Check `samples`, `window_ms` and `overwritten`; summaries describe retained
samples only. A raw export may be relatively large but is always bounded by
1,024 frames, independent of enemy population. Reporting and capture can perturb
the following frame: measure first, export afterward, and reset before a new trial.

## Timing boundaries

A frame interval starts at one redraw and ends at the next. The recorded work
belongs to the first redraw. The newest frame is pending until the next start,
so the first redraw after reset establishes a baseline rather than inventing a
sample. Inter-frame event handling and waits remain part of frame time.

| Field | Meaning |
|---|---|
| `frame_ms` | Unsmoothed redraw-start interval, including work and waiting |
| `simulation_ms` | Input preparation, the simulation tick loop, and per-tick effect observations |
| `game_step_ms` | Time inside Game::step calls, summed per frame; a subset of simulation_ms |
| `crowd_ms` | Grid rebuilding, candidate selection and crowd contact response; a subset of game_step_ms |
| `presentation_ms` | Camera, animation poses, instance/effect/debug preparation and HUD construction |
| `render_cpu_ms` | Renderer call and draw preparation, including surface acquisition, uploads, submission, present and any capture work |
| `other_ms` | Frame interval minus simulation, presentation and renderer CPU phases; includes event handling, reporting, title updates, recorder overhead and outside waits |

Each timing summary supplies mean, nearest-rank p50/p95/p99 and maximum in
milliseconds. `mean_step_ms` divides total Game::step time by executed ticks,
which distinguishes a costly tick from multiple normal ticks in one frame.
`max_ticks_per_frame`, `zero_tick_frames` and `multi_tick_frames` expose catch-up.
`mean_crowd_step_ms` divides crowd time by executed ticks. `crowd_candidates`
counts exact disc tests, while `crowd_possible_pairs` counts all unordered
non-player pairs before filtering. Both are summed across the same retained
ticks, including props, and also appear on each raw frame. Their ratio measures
how much distant-pair work the [crowd grid](crowd-collision.md) removes.
No-tick and empty windows report zero for means/rates that have no denominator;
check sample and tick counts before interpreting them.

`presented_fps` and `ticks_per_second` use the same retained interval duration.
`surface_skipped` counts redraws that could not obtain/present a surface; it is
not a count of missed display deadlines. Skipped redraws remain in the window
so occlusion cannot masquerade as successful high-throughput rendering.

The diagnostic target is explicitly 60 Hz, independent of simulation frequency
or actual display refresh. `over_budget` counts intervals longer than 1/60 s;
small vsync jitter can cross that threshold. `over_1_5_budget` counts intervals
longer than 25 ms. Neither field proves physical display frame delivery.

**Renderer CPU time is not GPU execution time.** CPU calls can block on vsync,
surface availability or the GPU; a long renderer phase does not isolate which.
`gpu_timing_available` is false. Crowd separation has its own timing boundary;
other simulation passes are included only in total step time. Read-only observer
callbacks mark boundaries in sim; the actual clock stays in app. The unpaced headless benchmark independently measures simulation without presentation.

The scene report exposes actual character draw counts as
`player_character_draws` and `horde_character_draws`: one draw per material
primitive per occupied pose, independent of how many enemies share that pose.
Character asset reports include parts, materials, unique images and total decoded
texture bytes including mips. The benchmark fingerprint uses these resource
counts and image dimensions so a multipart asset change cannot silently compare
against a different rendering workload.

## Reset and lifetime

`perf reset` immediately clears retained and pending samples without changing
Game, inputs, simulation clock, frame counters or scheduled work. Storage is
reused. A successful scene start/restart, additive scene load or eviction,
character/horde replacement, vsync change, or resize also clears the window.
Rejected operations preserve measurements. Ordinary spawns, defeats and combat
do not reset it; raw rows retain their population so changes can be correlated.
After changing other conditions, such as camera zoom or collision debug, reset
explicitly once the workload is ready.

## Repeatable benchmark suite

Ordinary verification runs due headless benchmarks and reports outstanding live
coverage. Reserve an uninterrupted foreground session before explicitly requesting
live measurement through `python3 scripts/verify.py --only performance-live` (which
also records the verification checkpoint), or the direct runner below.

Run the maintained runner with Python 3. Live mode builds the current checkout in
release, launches its own app and private harness, keeps the display awake, requests window
focus, and cleans up only its own processes. It refuses to measure while another
running `arpg` process competes with it; it does not stop someone else's game.
Keep the game visible and leave its input and window alone during measurement.

```sh
# Fast simulation feedback, unpaced and without a GPU/window:
python3 scripts/perf.py run --mode headless --output target/perf/candidate.json
# Integrated presentation/frame pacing (keep the game visible):
python3 scripts/perf.py run --mode live --output target/perf/live.json
# Explicitly publish a measurement for reuse by later commits:
python3 scripts/perf.py archive target/perf/candidate.json
# Compare explicitly or rerun only uncertain workloads:
python3 scripts/perf.py compare baseline.json candidate.json --output comparison.json
python3 scripts/perf.py run --mode headless --rerun candidate.json --output retry.json
```

Outputs are created exclusively: choose a new name for every run. There is no
`--bless` or automatic baseline replacement. `archive` validates the raw result
and creates an immutable compressed artifact plus a small provenance manifest in
[`benchmarks/evidence`](../benchmarks/evidence/README.md). Commit both with the next
change; the manifest identifies the measured revision and source digest, not the
later commit carrying the evidence. This evidence directory is excluded from the
source digest, avoiding the self-referential commit problem. Build/app logs remain
adjacent to the original run output and are not required for timing revalidation.

Every structurally valid run automatically saves an immutable compressed copy and
manifest in ignored `target/perf/evidence`. This includes noisy measurements,
without changing their verdicts. Later runs discover these copies automatically;
there is no need to remember an archive command for reuse on this machine.
`archive` publishes selected evidence into version control for reuse in other
checkouts. Failed/incomplete runs remain at their requested output and are never
registered as baseline candidates. Local storage is excluded by Git ignore rules
and does not invalidate a verification run.

Without `--baseline`, the runner searches both stores and selects the nearest ancestor with compatible
machine/compiler/build, workload files and policy **before building or measuring**.
It filters manifests by evidence schema, mode, source eligibility and workload
before reading raw artifacts. Eligible artifacts are hash checked and revalidated;
manifests cannot override their contents. Corrupt eligible evidence fails closed
during this preflight. Unrelated evidence cannot break another benchmark mode.
To seed a baseline for future changes, measure a clean committed checkout, then
archive and commit that evidence in a follow-up commit. A pre-commit measurement
remains dirty evidence even after its source is committed.

Dirty measurements remain usable explicitly; automatic reuse requires a
clean measured tree or an identical source digest, because a dirty tree's HEAD
does not prove its edits are ancestors. Missing compatible evidence is recorded as
`relative_status: missing`; an absolute pass alone proves no relative improvement.
Explicit incompatible comparison remains inconclusive while preserving absolute
budget results. An occupied result/log path or invalid explicit baseline is refused
before expensive work. A run also refuses known explicit mode, conditions or workload
mismatches during preflight; actual presentation settings and gameplay outcomes
are checked after collection. Failures during assessment or archiving preserve
collected trials and any completed absolute assessment at the requested output,
with an infrastructure failure and an inconclusive overall verdict. Saved raw
trials can be compared again without measuring again.

Evidence schema 2 requires living/corpse population telemetry and appears in both
manifest and result. Validation-contract changes must update the version.
Unsupported/unversioned manifests enter `baseline_exclusions` without opening
their artifacts; explicit use is rejected. Missing compatible evidence never
counts as a relative pass.

[The suite](../benchmarks/suite.json) owns workload selection, warm-up/sample ticks,
three repetitions, absolute budgets and comparison tolerances. It covers sparse
idle enemies, a compressed seeking crowd, movement with attacks, and a moving
animated crowd before compression. Scenes use the ordinary content loader; the
runner neither implements physics nor generates a second scene format. Use
`--workload sparse` (repeatable) for a focused run. A focused selection can reuse the matching workloads of a full baseline. `--vsync off` is available for throughput investigations; high frame
rates can exhaust the bounded recorder, making that sample inconclusive. Shorter
sample windows require an explicit suite edit, shared by both compared runs.

Trials drive the ordinary Game step with world-space intent at exact ticks. The
harness command `perf trial <warmup ticks> <sample ticks> idle|combat` restarts the
selected scene, then replies only after the final measured frame interval closes.
Warm-up accepts 1..3600 ticks and sampling 60..600 ticks. Combat walks a cardinal
square, changing direction every 60 sample ticks and asking for an attack every
30. The [shared trial schedule](../benchmarks/driver.rs) defines that schedule and its
version; this drives gameplay intent, not OS input or the keyboard binding table.
Both modes stand still during warm-up. Each repetition starts from the selected
scene again, so combat deaths do not reduce the next repetition's workload.

Warm-up and sampling end at exact tick boundaries. A boundary frame discards any
remaining catch-up ticks; all interior frames use the normal accumulator and
catch-up policy. Measurement resets on the first sampling frame and includes the
last sampling frame's interval. Hashing and serialization happen afterward.
Trial state is app-owned, outside simulation hashes; restarting cancels it and
refused requests preserve it. Native keys, clicks or resizing invalidate a trial.
The regular frame recorder also counts unfocused and occluded frames.

### Interpreting the result

Exit codes are part of the agent interface:

| Exit | Status | Meaning |
|---|---|---|
| 0 | `pass` | All applicable budgets and comparison checks passed |
| 1 | `regression` | Valid, stable evidence exceeded an absolute or relative limit |
| 2 | `inconclusive` | Missing, invalid, incompatible, unstable or borderline evidence |

Every result records revision, dirty/source fingerprint, executable fingerprint,
compiler/build settings, hardware/OS, scene/asset fingerprints, actual GPU/surface
settings, resolution, refresh rate, raw frames and trial outcomes. Source or binary
changes during measurement invalidate the run. Comparison revalidates raw evidence
rather than trusting a previously stored status.
Gated timing summaries must agree with their raw frame rows within the reporter's
rounding precision; contradictory evidence is inconclusive. Frame indices define
chronology even if a JSON formatter reorders object keys.

A valid trial needs release mode, a complete contiguous tick window, at least 30
completed frames, no overwritten/skipped/unfocused/occluded frames, expected
animation activity, and finite state. Repetitions must agree on final hash and
population observations. Comparisons require matching workload/policy, assets,
hardware/compiler/settings and gameplay outcomes. Hash equality is a conservative
compatibility check on this machine, not a cross-platform replay promise.

Fixed-population trials must retain every enemy. Combat must actually defeat
bodies; reports retain starting/ending counts and the sum of population across
sample ticks. Those observations must match the baseline. This compares a
repeatable combat sequence; it does **not** claim a constant-population combat
capacity.

The policy compares mean Game step cost, mean presentation preparation, and p99
frame interval. Renderer CPU time is retained diagnostically but is not a relative
gate: with vsync, faster simulation can legitimately increase time waiting inside
the renderer. GPU execution is still unmeasured.

Assessment is local to each workload and metric. A noisy combat presentation
metric cannot invalidate stable step/frame metrics or another workload. Invalid
raw evidence invalidates its workload, and still prevents overall certification.
Results contain `absolute_status`, `relative_status`, metric rows and a
`rerun_workloads` selection. Reruns create separate evidence, preserving healthy
measurements and never silently replacing samples with a more flattering range.
Gameplay/hash mismatch marks relative comparison incompatible; absolute budgets
are still evaluated. Review changed behavior before explicitly publishing its
measurements as new reference evidence.

Repeated metric ranges wider than the suite's allowed spread are inconclusive.
All repetitions must fall below an absolute budget to pass it; all must exceed it
to establish a regression. Relative pass requires the candidate's slowest trial
within tolerance of the baseline's fastest; regression requires the candidate's
fastest beyond tolerance of the baseline's slowest. Overlap is inconclusive.
Inspect uncertain ranges and repeat under stable conditions; never widen a gate
or replace a baseline just to turn it green.

## Agent maintenance contract

Keep bounded coarse telemetry, representative workloads, correctness oracles and
allocation checks permanently. Add detailed timers or profiler captures for a
specific investigation; retain a probe only when it answers an ongoing question.
Do not grow instrumentation without checking its overhead.

The headless runner (`scenario` package's `perf-headless` binary) loads the same
scene and uses the same validated tick schedule as the app. It records every
sample tick's Game::step duration in integer nanoseconds. Warm-up, hashing,
serialization and population observations sit outside that timing boundary.
It validates exact sample count/end tick, finite state, population outcomes and
repeatable hashes. It measures only `mean_step_ms`, using the existing suite's
step budget. Presentation metrics and frame budgets apply only to live mode;
headless evidence cannot establish rendering capacity.

[Verification policy](verification.md) defines when benchmarks run. A performance
claim must report the workload, before/after evidence, status/uncertainty and
gameplay verification. Run the full suite before claiming an engine-wide capacity
improvement. Intentional behavior or workload changes need reviewed new baseline
evidence, not automatic acceptance of a mismatched comparison.

The existing headless scenario budgets remain inexpensive smoke gates and verify
replay and predicted behavior. Live trials measure frame pacing and integrated
cost; they do not replace those correctness gates or establish combat feel.
The runner's evidence-policy tests execute inside `cargo test --workspace`; they
can also run directly with `python3 -B -m unittest discover -s scripts -p test_perf.py`.
