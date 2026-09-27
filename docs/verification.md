# Verification evidence

The completion command is:

```sh
python3 scripts/verify.py full --snapshot
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

Run focused checks while editing, then `full --snapshot` for completion. Hooks use this
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

## Verify while work continues

`--snapshot` captures the complete current checkout, including dirty files, new
unignored files and deletions. It checks for changes during capture and refuses
an inconsistent copy. Once it prints `Captured snapshot`, editing can continue:
checks run against the captured files, and changes to those files still invalidate
verification. Without this flag, checks run in place and require unchanged inputs
throughout the run.

The report's `snapshot.source` identifies the verified inputs; `live_checkout.matches`
says whether the live files still matched when verification finished. A mismatch
does not invalidate snapshot coverage, but the newer checkout remains unverified.
`live_checkout.changed_paths` lists the differences. Each run saves `snapshot.json`
with its input manifest and a hashed `source.tar` of the captured files alongside
the usual logs. The revision is context, not a claim that only committed files ran.

The isolated checkout lives at `target/verify/snapshot/workspace`, with its own
reusable build cache at `target/verify/snapshot/build`. Its first build is cold;
later runs reuse that cache without competing for the live checkout's build
directory. One snapshot run at a time owns this workspace and cache; a second run
refuses immediately. Historical source archives remain after the next capture
replaces the workspace. Unmerged index entries and absolute/external source
symlinks are refused because they cannot form this isolated candidate.

Snapshot mode isolates source and build files, not machine load. Continue editing
during correctness checks, but keep benchmark sessions free of competing builds
and other heavy work. Live benchmarks still require their explicit foreground flag.

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

Tests own their setups; playable `scenes/` files are not automated test inputs.
Prefer minimal independent system cases and a small set of complex test-only
compositions for interactions. The scenario schema accepts `GameScene` values
directly and has no external scene-file loading. Benchmarks likewise use only
their own workload scenes under `benchmarks/`.

[The suite manifest](../scenarios/suites.json) explicitly assigns expensive cases
to the extended suite; every other `.ron` is fast. Full verification runs both.
Missing manifest files fail, and no filename heuristic silently drops coverage.
Preserve distinct system assertions when consolidating tests; playable layout
and feasibility assertions are intentionally outside automated coverage.

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
