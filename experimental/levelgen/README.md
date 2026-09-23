# Level generation and navigation research

An independent Rust workspace with no Rust dependencies, plus a Python standard
library experiment runner. It generates ASCII maps, plays them through limited
observations, and searches for recipes that improve a fixed navigation benchmark.
It has no game, graphics, enemy or combat dependencies.

## Try it

From the repository root:

```sh
cd experimental/levelgen
cargo run -- --seed 42 --stages
cargo run -- play --seed 42 --width 40 --height 24 --min-route 15
cargo run -- play --manual --seed 42 --width 40 --height 24 --min-route 15
cargo run -- --help
```

Generation uses `#` rock, `.` floor, `S` start and `E` exit. Automatic play prints
both player routes: `*` visited once, `+` revisited. Manual play accepts `north`,
`south`, `west`, `east`, `observe` and `quit` on stdin. Its view uses `@` for the
player and `?` for unknown cells. Both people and agents can drive that interface.
A blocked move consumes a turn; observation does not. EOF before completion
returns failure, while `quit` explicitly ends inspection.

Run an autonomous parameter search, without any LLM connection:

```sh
python3 research.py --output runs/first --experiments 8
cat runs/first/report.md
cargo run -- play --recipe runs/first/best-recipe.txt --seed 42
```

Each output directory must be new. The runner builds and copies a release binary,
records source/binary SHA-256 hashes, establishes a baseline, and evaluates bounded
proposals. The built-in proposer changes one recipe field by its search step,
keeps improvements and explores neighbors of the new incumbent. It stops at the
experiment budget or when all available neighbors have already been evaluated.
It is a simple hill climber, not a language model.

## Use a coding agent as the researcher

An optional adapter uses an installed, authenticated Codex CLI:

```sh
python3 research.py --output runs/agent-first --experiments 8 --timeout 180 \
  --proposer "python3 $(pwd)/codex_proposer.py"
```

The adapter invokes a fresh `codex exec` with a read-only sandbox, ephemeral
session and a structured output schema. It uses your configured model and
account; each proposal consumes normal model usage. Its command interface is
based on the [official noninteractive documentation](https://developers.openai.com/codex/noninteractive/).
The outer runner's timeout covers the proposer and its child processes.

Any trusted local program can implement the same protocol. `--proposer` is parsed
as an argument list, never run through a shell, and invoked from the experiment
directory. Use an absolute script path. It receives one JSON context on stdin and
must return exactly one JSON object on stdout:

```json
{"hypothesis":"A denser initial cave field may reduce excess exploration.",
 "recipe":{"rock_percent":48,"smoothing_passes":5,"carve_percent":45,"straight_percent":0}}
```

The context supplies the authoritative recipe schema, fixed scoring objective,
selection rule, incumbent recipe and measurements, recent decisions, and paths
to inspect maps and traces. See [program.md](program.md) for the agent instructions.
The runner refuses unknown/missing fields, out-of-range or non-integer values,
duplicate JSON keys, duplicate recipes, non-finite values and oversized responses.
Malformed proposals and process failures consume an experiment and are logged;
they never receive evaluation credit. Protected source/binary changes abort the
run. Run directories are never reset or overwritten.

The supported search surface is **recipe values only**. There is no generated
code execution or self-editing evaluator. The generic proposer command is a
trusted local executable, not an OS sandbox; source hashing detects changes but
does not make arbitrary commands safe. The Codex adapter additionally requests
read-only tool execution. Agent code evolution can be a later experiment with
its own process boundary.

## What stays fixed

The [`Request`](src/generation.rs) owns map requirements: size, passage width,
open-area range, minimum start-to-exit distance and attempt budget. The CLI uses
its defaults. [`Recipe`](src/recipe.rs) owns algorithm settings, their validation,
and the schema consumed by proposers. Print a complete default recipe with:

```sh
cargo run -- recipe > cave.recipe
cargo run -- --recipe cave.recipe --seed 42
```

All four fields are required. Cave generation uses the initial rock percentage
and number of synchronous smoothing passes. Tunnels use the target carved
percentage and probability of maintaining the previous digging direction.
The same recipe is tested on both styles. Recipe changes cannot alter the
benchmark's requests, observation rules, players, seed lists or scoring.

The recipe builds a coarse grid. The fixed generator layer expands each floor
cell into a full `passage-width` square. Adjacent squares share a complete edge;
a solid outside border and unused right/bottom remainder are retained. This gives
chunky but predictable passage widths. Endpoint markers are diagnostic cell
positions, not validated spawn positions for an actor with physical radius.

Caves smooth neighbors, then discard all but the largest connected region.
Tunnels use a bounded random walk. Acceptance measures the finished map: nonempty
connected floor, distinct endpoints, exact open-area bounds and minimum route
length. The endpoint heuristic uses two farthest-cell searches; the measured
route is an exact shortest path, not a promise of the map's global diameter.

All distances are cell steps. Generation failures print a diagnostic map and
return status 1; invalid input returns 2. Research suite evaluation successfully
writing a report returns 0 even when that report includes failed maps.

## What the bots know

[`Session`](src/play.rs) owns the complete level privately. Its observation
contains only position, remembered cells and the exit **after discovery**. The
policy receives that observation, with no map reference or diagnostic traces.
Local sensing follows up to three floor edges, revealing walls reached along
the way. This can reveal around corners; it is not optical line of sight.

- **Seeker:** heads to the exit once discovered; otherwise visits the nearest
  reachable frontier between known floor and unknown cells.
- **Explorer:** continues visiting frontiers until all floor is discovered,
  then heads to the exit.
- **Reference:** the evaluator computes the omniscient shortest route. This is
  a comparison measurement, not hidden knowledge available to either bot.

Each playthrough is bounded by six times the output cell count. A bot that hits
the limit or cannot choose an action before its goal counts as an incomplete
playthrough. Completed sessions refuse further moves. Stable traversal order,
seeded generation and step-count budgets keep runs repeatable.

## Evaluation and selection

To evaluate one recipe directly (the parent directory must exist):

```sh
mkdir -p runs
cargo run --release -- evaluate --suite development --output runs/baseline
```

[`evaluation.rs`](src/evaluation.rs) defines three disjoint public seed suites:
development, validation and final holdout. Each has 16 cases spanning both
biomes and two map sizes. Reports include the exact seeds, dimensions, generation
attempt counts and fixed objective. The research runner never sends holdout
results to its proposer or uses them to select a recipe. It evaluates baseline
and selected recipe on holdout only after search stops. These seeds are reserved,
not secret; repeatedly tuning against final reports defeats their purpose.

The score starts at 1000 and subtracts weighted distance outside two target
bands: seeker walking distance divided by the optimal route, and the fraction
of explorer steps that revisit a position. The exact bands and weights are
owned by the evaluator and included in its report. Scores are clamped to 0..1000;
invalid maps and incomplete plays receive zero. All cases remain in the average.
These are inspectable navigation preferences, not an objective definition of fun.

A candidate must improve development mean by at least one point without worsening
its worst case, generation failures or incomplete plays. Only then is it tested
on validation, where none of those measurements, including the mean, may regress.
The final holdout reports generalization; it does not trigger another selection
or silently roll back the winner. A failed holdout gate is reported explicitly.

## Evidence

Every run saves:

- `manifest.json`, `source/`, `evaluator`: exact source snapshot, binary and hashes.
- `baseline/`: baseline recipe and development/validation reports, then final holdout.
- `experiment-NNN/`: proposal context, hypothesis, recipe, decisions and evaluations.
- `experiments.jsonl`: append-only decisions, including crashes and refused proposals.
- `best-recipe.txt`, `result.json`, `report.md`: selected recipe and before/after results.
- `final/holdout/`: final selected-recipe holdout measurements.

Every evaluated case retains its ASCII map. Accepted cases retain per-player route
overlays and action traces with tick, action, position, newly revealed floor,
revisit and blocked-move status. Per-experiment results identify the largest
improvements and regressions relative to the incumbent. Overlays are diagnostic
full-map views, not the observations shown to bots.

To replay using the exact saved binary:

```sh
runs/first/evaluator play --recipe runs/first/best-recipe.txt --seed 42
```

Root workspace checks do not include this independent experiment. Run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --bins --no-deps --document-private-items
python3 -m unittest discover -s tests -p 'test_*.py' -v
```

Tests include hidden-information boundaries, predicted discovery steps, blocked
moves and time limits, different player goals, generation invariants, rejected
inputs, selection regressions, process timeouts, source/binary integrity checks,
and a complete run through an external local proposer. The Codex adapter also
has a CLI fixture test that makes no model calls.
