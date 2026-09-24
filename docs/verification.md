# Verification evidence

Run one command from the repository root:

```sh
python3 scripts/verify.py quick
python3 scripts/verify.py full
python3 scripts/verify.py full --live
```

`quick` is explicitly partial feedback: Clippy, workspace tests, and the small
scenario suite. `full` additionally checks private Rust documentation, extended
scenarios, and unpaced release Game performance. `--live` adds the real-time
presentation benchmark; headless evidence never certifies frame pacing or GPU
behaviour. The runner prints omitted or uncertified checks and writes a structured
result under `target/verify/run-*/verification.json`. Exit 0 means the requested
checks passed, 1 means a failure, and 2 means inconclusive evidence. Read `complete`
and coverage before treating a quick or `--only` run as a completion gate.

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
