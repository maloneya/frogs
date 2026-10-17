# Physical-door scalability audit — 2026-09-27

The converged contact solver cost **125 ms per tick** once the 10,000-actor
benchmark crowd compressed. The [soft step](crowd-collision.md#soft-step) replaced
it. This page records what each finding became and which costs remain bounded
rather than removed.

## Measurements

These exploratory release measurements were collected on 2026-09-27 on one M4
thread using the benchmark-owned `sparse.ron` and `compressed.ron`, with 10,000
enemies retained. They established no live rendering coverage. They predate
subsequent actor, asset and renderer changes and do not certify the current
checkout; use the [accepted ceiling](performance.md#accepted-performance-ceiling)
and [verification reports](verification.md) for current evidence.

| Workload | Before | After |
|---|---:|---:|
| Compressed, idle (actors pursue the player) | 125.0 ms | 3.6–3.7 ms |
| 900 coincident bodies, worst sampled tick | — | 36 ms |
| Compressed, scripted combat | — | 3.7–4.0 ms |
| Slam into the packed crowd | 14–19 ms | 4.5 ms mean, 9 ms worst tick |
| Sparse | 0.95 ms | 0.54 ms |

The compressed crowd now runs one collision pass of about 29,000 contact rows and
four substeps. The [profiling procedure](performance.md#detailed-physics-investigation)
reports the new phases and counters.

## Findings

1. **Repeated global closure and solve — resolved.** Contacts are collided once
   per pass and solved with fixed work per substep. Nothing iterates to a
   tolerance or restarts.
2. **Population-wide recovery sweeps — resolved.** Overlap correction is part of
   each substep's soft solve; there is no separate recovery or repair state.
3. **Population-sized bitsets in grid queries — resolved.** The
   [grid](../crates/sim/src/grid.rs) is a counting sort rebuilt once per pass and
   visits each nearby pair once.
4. **No spatial filtering for walls and panels — resolved for bodies.** Walls
   keep a per-cell index rebuilt when the wall set changes, and panels query the
   body grid. Panels still check every wall with an AABB prefilter, bounded by
   64 × 256 per pass. Cover rays and the continuous sweep still test every wall
   and panel, but only per query or per swept body.
5. **Bulk deaths rebuild contact history — resolved.** Revocation is O(1) per
   body; stale forces are filtered immediately and dropped at the next step.
6. **One extreme velocity widens every query — resolved.** Only bodies fast
   enough to need extra passes, and bodies near their swept paths, are
   re-collided, though each extra pass still rebuilds the whole body grid. Beyond eight passes (about 77 m/s) a continuous sweep stops
   bodies at walls, the arena edge and panels. Body-to-body tunnelling remains
   possible only above that speed.
7. **Dense overlap defeats the grid — bounded by admission.** A coincident pile
   still produces every pair in a cell. Warm-start lookup once walked each
   body's remembered contacts, so a pile of k bodies cost k³: 183 ms on the
   worst tick of a 900 pile. An open-addressed key index removed that term; the
   same tick now costs 36 ms for about 400,000 pair rows. Formations now refuse
   spacing below a body diameter. Explicit placements are bounded by placement
   capacity. A source emitting every tick disperses its births and stays near
   0.15 ms. There is deliberately no contact cap.
