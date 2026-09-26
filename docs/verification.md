# Verification evidence

The completion command is `python3 scripts/verify.py full`. It runs Clippy,
workspace tests, private Rustdoc, native tuning companion tests/build and all scenarios with replay. Release benchmarks
are required every **10 relevant commits**, and agents may run them sooner when
useful (for example, investigating a slowdown or validating an optimization).

The runner counts commits since each mode's last successful checkpoint on the
current Git ancestry. Relevant paths are defined once in
[`scripts/verify.py`](../scripts/verify.py): runtime crates, assets, scenes,
scripts and build inputs; Markdown edits are excluded. Dirty edits are tested
but do not increment the commit counter. A dirty run checkpoints its HEAD, so
its eventual commit still counts toward the next checkpoint.

Headless and live modes have independent counters. Missing, damaged or unavailable
checkpoint evidence makes that mode due immediately. Failed or inconclusive runs
do not advance the checkpoint. Checkpoints are local evidence under `target/verify`;
a new checkout or removal of that evidence requires an initial benchmark run.
The runner records the counters and required checks in its result. Benchmark
samples are fresh whenever a mode is selected; this cadence is not a claim that
intervening changes have measured performance coverage.

```sh
python3 scripts/verify.py quick              # partial correctness feedback
python3 scripts/verify.py full               # correctness + any due benchmarks
python3 scripts/verify.py full --performance # agent discretion: both modes now
python3 scripts/verify.py full --live        # agent discretion: live mode now
```

`quick` runs Clippy, workspace tests and the small scenario suite; it cannot
certify completion. `--only` likewise selects an explicitly partial check.
Headless benchmarks measure Game step cost; live benchmarks measure real-time
presentation. Headless evidence never certifies frame pacing or GPU behaviour.
Existing scenario timing assertions remain correctness smoke gates.

The runner prints omitted required checks and writes a structured result under
`target/verify/run-*/verification.json`. Exit 0 means the requested checks passed,
1 means failure, and 2 means inconclusive evidence. Read `complete` and coverage
before claiming completion. A complete result certifies the required checks for
this change; benchmarks are included only when due or explicitly requested.
Never widen a budget or replace a baseline merely to clear a failure; see
[performance evidence](performance.md#agent-maintenance-contract) for measurement
and baseline requirements.

All scenarios still run and replay in full verification. The explicit
[extended manifest](../scenarios/suites.json) contains population-scale and long
encounter cases with a reason for each. Every other `.ron` file belongs to the
fast suite, including newly added scenarios. Manifest entries naming missing files fail;
there is no filename heuristic that silently drops new coverage. Removing a
scenario requires preserving its distinct failure coverage elsewhere, not merely
meeting a target count.

## Reusing successful checks

Successful correctness checks are keyed by the complete tracked and unignored
source/data contents, command, compiler, machine and build settings. Dirty and
new files participate; an old commit name cannot disguise an edited checkout.
Each cache entry points to original logs and evidence with integrity digests.
Missing or changed evidence runs the check again. Failures and source changes
during a check are never cached as passes. `--fresh` forces execution.

This deliberately uses a conservative whole-checkout key rather than a guessed
change-impact map. The first change invalidates correctness evidence; repeated
calls by hooks, agents or a final report reuse the same verified result. Performance
measurements are not cached by this layer. Their independent baseline selection,
compatibility and local metric assessment belong to the
[performance runner](performance.md#repeatable-benchmark-suite).

`--only clippy` is an explicitly partial entry point for edit hooks. The stop hook
runs `full` and benefits from already completed checks. These hooks are one client
of the same runner, not a separate definition of what verification means.

## Finding expensive scenarios

Both scenario suites save per-scenario timing evidence. For a focused command:

```sh
cargo run --quiet -p scenario -- --timings /tmp/scenario-timings.json scenarios/walk_east.ron
```

The output path must be new. Timings distinguish first-run setup, simulation,
hashing, assertions, other runner work, and the entire replay. They are observation
costs, never simulation inputs, and cannot change replay hashes. The existing
`max_mean_step_micros` assertion still measures only `Game::step`. Total elapsed
time includes parsing and reporting too, so it need not equal the listed phases
exactly. Use these measurements to choose a cheaper fixture or remove duplicate
coverage; do not weaken a correctness assertion to make a slow test green.

## Events and identities

`Game::set_trace_retention` controls recent history; zero disables it. Capture
limits and retention are validated against the engine-owned `MAX_TRACE_EVENTS`;
rejected limits preserve existing history and capture.

Scenario spawn bindings use authoritative request tickets and completed-tick
outcomes. Diagnostic retention cannot change which entity an assertion names.
Event assertions select typed kinds and require the exact ordered sequence within
that scope. An unrelated gait event does not invalidate a source-cadence test;
an extra source firing does. Selected-event capture is independently bounded and
fails explicitly on overflow, even if the live debug ring is disabled or cleared.
See the [scenario guide](../.claude/skills/scenario/SKILL.md) for authored syntax.
