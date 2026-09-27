# Traps

Search by observable symptom before diagnosing. Add an entry after a misleading
failure costs more than ten minutes: cause, check, fix and date. When a failure
recurs, enforce its prevention at the owner or in a regression test. Remove
advice that the new safeguard makes unnecessary; retain operational limitations.

## Screenshot times out or the image is black

**Cause:** capture depends on a presented surface. A fully covered window or a
sleeping display may stop presentation. An unfocused but visible window can work.
**Check:** compare rendered frame counts across a harness wait.
**Fix:** uncover the game and wake the display; use the harness capture command.
The capture owner rejects overlapping requests and reports write failures or a
four-second timeout. Capture is not independent of window visibility.

## Native companion capture is black or omits the sidebar

**Cause:** AppKit view captures leave undrawn regions transparent; macOS glass
navigation sidebars can omit their composited content from `cacheDisplay` even
when their native table has rows. This is a capture limitation, not evidence of
an empty tuning catalog (2026-09-26).
**Check:** inspect the PNG alpha and the game's tuning `state` reply.
**Safeguard:** companion self-capture composites over the resolved native window
background. The small settings view uses a standard `HSplitView` and plain list,
which also avoids unnecessary navigation containers and captures its rows.

## Swift reports that FileManager has no member default

**Cause:** switching between checkouts that share `target` can reuse Swift module
caches containing paths from the other checkout (2026-09-26).
**Check:** the same source builds in a fresh Swift scratch directory; adding an
explicit Foundation import does not resolve the stale-cache failure.
**Safeguard:** `scripts/companion.py` namespaces its scratch directory by the
canonical checkout path, so worktrees sharing Rust artifacts keep separate Swift caches.

## Frame rate is absurd, zero, or faster for a heavier scene

**Cause:** occlusion can skip draws; display sleep can stop presentation;
macOS can throttle a background window intermittently.
**Check:** compare rendered frames, skipped frames and simulation ticks over the
same interval. Repeat with the window visible and frontmost. A small gap between
vsync and uncapped rates can also mean a real workload bottleneck.
**Fix:** follow the [measurement procedure](../.claude/skills/playtest/SKILL.md).
Report conditions and sample variation. Use the scenario runner's step-time
budget to isolate simulation cost from presentation cost.

## Injected keys do nothing or reach another application

**Cause:** OS key injection depends on focus and permissions.
**Fix:** drive the game through `ARPG_HARNESS`; it uses the real binding table
without OS input delivery. Capture still has the visibility restriction above.

## A position streaks for one frame, especially uncapped

**Cause:** a frame can run zero simulation ticks. A position changed between
ticks may be drawn using an old previous-position snapshot.
**Check:** compare interpolation endpoints before another tick runs.
**Fix:** position replacement must update both snapshots at the owning boundary.
`Bodies::spawn` already does this. Velocity-only changes must not rewrite them.
Test a new position-changing operation with no intervening tick.

## Bodies jump to an arena corner

**Cause:** non-finite solver output may be hidden by clamping arithmetic.
**Check:** run a debug build; containment checks finiteness before clamping.
Inspect coincident-body normalization upstream of containment.
**Safeguard:** contact handles coincident bodies; containment has debug assertions;
the scenario runner checks final finiteness in all builds. Debug assertions are
absent in release, and a final check cannot detect poison already hidden earlier.

## A grid rejects most pairs but the packed crowd is slower

**Cause:** fewer exact tests do not guarantee cheaper candidate discovery.
Repeated sorting, re-scanning cells as a body oscillates across a boundary,
and one-pointer-per-body bucket traversal can outweigh the discarded tests.
**Check:** measure the entire crowd pass after pursuing actors fully compress, not just
candidate counts or a short run of the initial spawn layout.
**Safeguards:** the crowd owner uses ordered bitsets, scans each cell once per
outer row, and stores indices in contiguous pages. Brute-force equivalence tests
cover returning across cell boundaries; the dense-crowd scenario runs through
compression with a step-time budget. See [crowd collision](crowd-collision.md).
