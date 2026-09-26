# Immutable performance evidence

`perf.py run` automatically retains immutable evidence in ignored
`target/perf/evidence` and later runs discover it there. To publish a measurement
for other checkouts, `python3 scripts/perf.py archive RESULT.json` writes a content-addressed `.json.gz`
raw artifact and a small `.json` manifest. Commit both. Never edit either file;
the runner checks raw SHA-256, provenance and measurements before reuse.

The measured revision and dirty source digest identify what ran, independently
of the later commit carrying the result. Archived dirty results are historical
observations, not automatically eligible baselines for other source trees.

Evidence schema 1 was retired when corpse telemetry became required. Those
artifacts were removed rather than assigning values to unmeasured fields; their
original observations remain in Git history. New evidence uses schema 2 in both
manifest and raw result. Old local manifests are explicitly excluded from
automatic discovery, and explicit old results are rejected.

See [performance policy](../../docs/performance.md) for selection,
absolute versus relative results, and the command to collect new evidence.
