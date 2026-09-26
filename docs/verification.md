# Verification evidence

The completion command is:

```sh
python3 scripts/verify.py full
```

It runs Clippy, workspace tests (including GPU tests), private Rustdoc, native
companion tests/build, every scenario with replay, and due headless benchmarks.
It never launches a live benchmark without an explicit flag. Headless/live release
benchmarks are independently due every **10 relevant commits**, or immediately
when checkpoint evidence is unavailable. Relevant paths and check commands are
owned by [verify.py](../scripts/verify.py); Markdown is excluded from benchmark
cadence. Dirty edits are checked but do not increment the commit counter; their
eventual commit does. Successful modes advance their own checkpoint only if
source stayed unchanged. Headless evidence cannot certify rendering performance.

## Choosing checks

| Command suffix | Coverage |
|---|---|
| `quick` | Partial: Clippy, workspace tests, fast scenarios |
| `correctness` | All correctness; reports due benchmarks as omitted |
| `full` | All correctness and due headless benchmarks; reports due live coverage as omitted |
| `full --performance` | Also force both benchmark modes; requires a foreground session |
| `full --live` | Also force live benchmarks; requires a foreground session |
| `--only <check>` | Explicitly partial selection |

Run focused checks while editing, then `full` for completion. Hooks use this
same runner; their presence does not prove execution. GPU tests require a real
adapter; scenarios need no GPU/window. Rustdoc is separate from Clippy/tests.

Live measurements are a deliberate foreground session: close other game instances,
keep the benchmark window visible and leave input alone. When ready, run
`python3 scripts/verify.py --only performance-live`, or include it with `full --live`.
Until then, due live coverage remains in `required`/`omitted`, with no checkpoint
advance. Otherwise successful automatic runs exit 0 as `partial_pass`; hooks do
not repeatedly seize the desktop. Rendering/frame-pacing performance claims still
require live evidence, even if cadence has not made it due.

Results live in `target/verify/run-*/verification.json`. Read `coverage`,
`omitted`, `complete` and source identity. Exit 0 means requested checks passed,
1 means failure, 2 means inconclusive. A partial pass cannot certify omitted
checks. Preserve failures and uncertainty; never widen a budget or bless a trace
to clear a gate. [Performance policy](performance.md#agent-maintenance-contract)
owns measurement and baseline requirements.

## Reuse and retry

Successful correctness checks are keyed by all tracked/unignored source and data
contents, command, toolchain, machine and build settings. New/dirty files count.
The whole-checkout key deliberately avoids guessed change-impact rules. Entries
link to integrity-checked logs/evidence; missing, damaged or changed evidence
reruns the check. Failures and checks spanning source changes never cache as passes.
`--fresh` forces correctness execution.

Performance samples are fresh whenever a mode is selected. An explicitly
classified infrastructure failure can defer automatic retries for identical
source, command, environment and baseline-store contents. Correctness still runs;
the result remains inconclusive, with no benchmark checkpoint or timing pass.
Changed inputs or damaged evidence trigger a retry. Use `--performance`, `--live`
or `--only performance-headless|performance-live` after repairing the condition.
`--fresh` does not override this deferral. Noisy timing, regressions and measurement
failures are never suppressed by it.

## Scenario coverage and evidence

[The suite manifest](../scenarios/suites.json) explicitly assigns expensive cases
to the extended suite; every other `.ron` is fast. Full verification runs both.
Missing manifest files fail, and no filename heuristic silently drops coverage.
Preserve a removed scenario's distinct assertions elsewhere.

Both suites save per-case timings. For a focused profile:

```sh
cargo run --quiet -p scenario -- --timings /tmp/scenario-timings.json scenarios/walk_east.ron
```

Choose a new output path. Timings distinguish setup, simulation, hashing,
assertions and replay; they never enter simulation. `max_mean_step_micros`
measures only `Game::step`. Use evidence to choose cheaper fixtures or remove
redundant checks without weakening predictions.

The [scenario guide](../.claude/skills/scenario/SKILL.md) owns tick/identity and
assertion syntax. Bindings use authoritative spawn tickets/completed-tick
outcomes, independent of trace retention. Selected-event assertions require
exact ordered sequences and reject overflow; unrelated event kinds do not fail
them. Diagnostic history is bounded by `MAX_TRACE_EVENTS`; zero retention disables
it, and rejected limits preserve accepted history/capture. Neither reports nor
replay substitute for predicted outcomes or human playtesting.
