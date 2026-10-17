# Immutable performance evidence

`perf.py run` automatically retains immutable evidence in ignored
`target/perf/evidence` and later runs discover it there. To publish a measurement
for other checkouts, `python3 scripts/perf.py archive RESULT.json` writes a content-addressed `.json.gz`
raw artifact and a small `.json` manifest. Commit both. Never edit either file;
the runner checks raw SHA-256, provenance and measurements before reuse.

The measured revision and dirty source digest identify what ran, independently
of the later commit carrying the result. Archived dirty results are historical
observations, not automatically eligible baselines for other source trees.

See [performance policy](../../docs/performance.md) for selection,
absolute versus relative results, and the command to collect new evidence.

The [2026-10-17 accepted ceiling](accepted-20261017.md) records the current
user-approved renderer, final workload trials, GPU diagnostics and explicit
baseline selection for these historical dirty-source measurements. It preserves
variance and does not certify every frame at 60 FPS.
