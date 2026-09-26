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
