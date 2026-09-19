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
callbacks mark boundaries in sim; the actual clock stays in app. The headless scenario step budget independently measures
simulation without presentation.

## Reset and lifetime

`perf reset` immediately clears retained and pending samples without changing
Game, inputs, simulation clock, frame counters or scheduled work. Storage is
reused. A successful scene start/restart, additive scene load or eviction,
character/horde replacement, vsync change, or resize also clears the window.
Rejected operations preserve measurements. Ordinary spawns, defeats and combat
do not reset it; raw rows retain their population so changes can be correlated.
After changing other conditions, such as camera zoom or collision debug, reset
explicitly once the workload is ready.
