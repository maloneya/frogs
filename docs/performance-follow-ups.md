# Performance follow-ups

Record observations using the [performance policy](performance.md#feature-work-record-and-continue).
Keep entries here for later investigation while completing the assigned task.
This log does not replace the accepted reference or raw benchmark evidence.

For each issue, use a descriptive heading and record:

- **Observed:** date, task/feature, changed paths/commits and measured source
  identity (including dirty digest or snapshot). State attribution limits.
- **Evidence:** measured regression, suspected concern or missing coverage;
  workload/metric, trials, baseline/limit if available, raw evidence and snapshot
  links. Separate established cause, hypothesis and unknowns.
- **Reproduce:** command, working directory and required conditions, or relevant
  code/conditions for an unmeasured concern.
- **Follow-up:** completed/omitted checks, practical impact and deferred question.
  Append later observations with their task/source identities; retain the history
  when an explicitly requested investigation resolves the issue.

Historical measurements remain in [benchmark evidence](../benchmarks/evidence/README.md).

## Player ability allocation invariant — 2026-10-17

- **Observed:** During the merge-review cleanup, the existing allocation invariant
  moved from sim into `crates/game/tests/player_abilities.rs` with player ability
  ownership. The Cleave case observed **4 allocations against a limit of 0**
  across three swing durations with 512 passive hostiles, after one warm-up tick.
  This is an allocation concern; neither its cause nor a release timing regression
  has been established. No pre-change Game measurement isolates this task's effect.
- **Evidence:** The [initial log](../target/merge-review-20261017/stopped/arpg-player-abilities-tests.log)
  and [stopped snapshot manifest](../target/merge-review-20261017/stopped/snapshot.json)
  retain the original observation, [tracked patch](../target/merge-review-20261017/stopped/working-tree.patch)
  and [untracked sources](../target/merge-review-20261017/stopped/untracked-files.tar.gz).
  The later canonical cleanup snapshot reproduced 4 versus 0 in
  [tests.log](../target/verify/run-64it78lo/tests.log): HEAD
  `4fdaf21d7252e2515beb71488bc9a29bcfcad877`, dirty snapshot source
  `8b93763271cf3b70f4ca7e5b8adfa305ff2f18c2af295dd3e86b07c149fc73f2`;
  [manifest](../target/verify/run-64it78lo/snapshot.json) and
  [source archive](../target/verify/run-64it78lo/source.tar) identify the inputs.
- **Reproduce:** From the restored source at the repository root, with Cargo
  available: `cargo test -p arpg-game --test player_abilities a_steady_state_frame_allocates_nothing -- --exact`.
  This describes a future requested reproduction, not authorization to retry.
- **Follow-up:** The zero-allocation assertion remains unchanged. Eight other
  player-ability correctness tests passed; this test stops at Cleave, so the
  failure provides no later-profile allocation result. Attribution, profiling,
  optimization and release benchmarks were deferred by the user. The completed
  [snapshot verification](../target/verify/run-64it78lo/verification.json) passed
  Clippy, Rustdoc, companion and both scenario suites; its workspace-test check
  failed on this unchanged assertion. Concurrent policy/verifier edits postdate
  that captured source and received separate focused tests.

## Staggered-horde debug scenario timing — 2026-10-17

- **Observed:** The intermediate merge-cleanup candidate's
  `a_horde_staggers_its_attacks` scenario observed **1462.31 microseconds mean
  Game step cost against its 1000-microsecond smoke limit**, over 600 ticks with
  eight actors. This was a debug run alongside concurrent builds. It is a timing
  concern, not an established release regression or attribution to the cleanup.
- **Evidence:** [Full scenario log](../target/merge-review-20261017/all-scenarios.log)
  and [per-case timings](../target/merge-review-20261017/all-scenarios.timings.json).
  All 78 scenarios' functional and replay predictions passed; this timing check
  was the sole reported failure. The intermediate dirty run's exact source digest
  was not retained, so it must not be identified as the later canonical snapshot
  above merely because both were based on the same HEAD.
- **Reproduce:** For a later requested reproduction, from the repository root:
  `cargo run --quiet -p scenario -- --timings /tmp/arpg-horde-follow-up.timings.json scenarios/a_horde_staggers_its_attacks.ron`.
  Use a fresh output path and disclose build mode and competing machine load.
- **Follow-up:** Preserve the smoke limit and raw observation. Cause and
  comparison to compatible release evidence remain unknown. Both scenario suites
  later passed during the already-running canonical verification of the final
  captured cleanup source. That result does not erase this intermediate noisy
  observation. No diagnostic timing retry or investigation was performed.

## Fresh accepted-ceiling coverage missing — 2026-10-17

- **Observed:** The merge cleanup changes gameplay ownership, corpse lifetime,
  character instance storage and runtime asset loading. Fresh **headless and live
  release benchmark coverage is missing** for the final cleanup source identified
  above. No release benchmarks ran during this task, following the user's
  deferral; correctness tests and debug scenario timings cannot fill that gap.
- **Evidence:** The [pinned reference policy](../benchmarks/accepted-ceiling.json)
  selects the immutable [2026-10-17 accepted measurements](../benchmarks/evidence/accepted-20261017.md).
  Their original source/binary provenance and noisy observations remain unchanged.
  The [verification transcript](../target/merge-review-20261017/completion-verification.log)
  records both modes due and live coverage omitted for the canonical snapshot.
- **Reproduce:** When benchmark work is explicitly in scope, use fresh result
  paths with `python3 scripts/perf.py run --mode headless --output target/perf/cleanup-headless.json`
  and `python3 scripts/perf.py run --mode live --output target/perf/cleanup-live.json`,
  from the measured repository root. Live mode requires the reserved foreground
  session and conditions in the [performance policy](performance.md#measuring).
- **Follow-up:** Relative performance, release budgets and live frame pacing
  remain uncertified. Do not replace the accepted references or widen tolerances.
  Missing/incompatible coverage and noisy reference comparisons remain
  inconclusive; no conclusion about the cause or size of any change is available.
