# Immutable performance evidence

`perf.py run` automatically retains immutable evidence in ignored
`target/perf/evidence` and later runs discover it there. To publish a measurement
for other checkouts, `python3 scripts/perf.py archive RESULT.json` writes a content-addressed `.json.gz`
raw artifact and a small `.json` manifest. Commit both. Never edit either file;
the runner checks raw SHA-256, provenance and measurements before reuse.

The measured revision and dirty source digest identify what ran, independently
of the later commit carrying the result. Archived dirty results are historical
observations, not automatically eligible baselines for other source trees.

The initial two artifacts preserve the actual pre/post horde-motion trials from
September 23, 2026. Their original assessments were inconclusive due noisy metrics;
archiving them does not turn them into passes. A subsequent headless measurement records the workflow implementation and passes
absolute step budgets; its dirty source digest limits automatic reuse to identical
inputs. See [performance policy](../../docs/performance.md) for selection,
absolute versus relative results, and the command to collect new evidence.
