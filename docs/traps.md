# Traps

Keyed by **observable symptom**. Grep this before diagnosing something
surprising; append after any symptom that cost more than ten minutes.

An entry here sits at layer 4 of the invariant ladder in `CLAUDE.md` — the
weakest tier there is. That is deliberate but temporary. **An entry that fires
three times is telling you it belongs at layer 0–3.** Promote it to a const
assert, a `build.rs` guard, a `#[must_use]`, a private field or a test, then
delete the entry and note the promotion. A trap file that only ever grows is a
file nobody reads.

The second use of this file is pre-execution checks: once a class of failure
recurs, stop looking it up and make it a validation that runs before the work.

Format: `## symptom:` / cause / check / fix / promoted-to, dated.

---

## symptom: `shot` reports success but the PNG is missing, or the image is black

*2026-01 — cost several hours across two sessions*

**Cause.** The window was fully occluded. An occluded surface hands back no
texture, the draw is skipped, and capture — recorded between drawing a frame and
presenting it — never runs. Unfocused is fine; *covered* is not.

**Check.** Diff `frames` across a `wait`. Flat means nothing is presenting, and
no screenshot will work until the window is uncovered.

**Fix.** Uncover the window. Do **not** switch to the `screencapture` utility or
any desktop screenshot tool; see the OS-automation entry below.

**Partly promoted.** `capture_has_stalled` now waits a few seconds of skipped
frames and replies with an error naming occlusion, so the failure is loud rather
than a silent `ok`. Fully promoting it means moving capture off the swapchain to
an offscreen render target — roadmap chunk 6.

---

## symptom: absurd frame rate — thousands of frames per second, with vsync on

*Observed once at 12,467 fps.*

**Cause.** Skipped frames counted as presented ones. The occluded window hands
back no texture, the draw is skipped, and the loop spins as fast as it likes
while reporting every spin.

**Check.** Diff `skipped` across the measurement window. A small constant from
startup is normal; a rising one is the tell.

A second, quieter version: a *backgrounded* window presents honestly but
throttled, with `skipped` at zero. Measured 63/s vsync and 65/s uncapped where
the same build with the window up gave 62/s and 302/s.

**Fix.** Measure both present modes and compare. If uncapped is not several
times vsync, the app is throttled and the number is not a measurement. Bring the
window frontmost and repeat. Never trust `frame_ms` alone — it is an EMA and
cannot distinguish a steady 60Hz from a mixture averaging to it.

**Promotion candidate.** A frame-time histogram instead of an EMA, and a
headless `step()` perf assertion that does not depend on a window at all
(roadmap chunk 5).

---

## symptom: injected keys do nothing, or land in a different application

**Cause.** Driving the game through the OS — synthetic keystrokes via
`osascript`, screenshots via `screencapture` — instead of through
`ARPG_HARNESS`. OS automation needs the window frontmost, the display awake and
accessibility permission, and when any of those is false it does not fail: keys
go to whatever *is* focused and images come back black. Both look exactly like
the game being broken.

**Fix.** Use the harness socket. Everything in the `playtest` skill works on an
unfocused window buried behind others.

**Promoted.** The `playtest` skill description names this explicitly so the
wrong tool is not reached for in the first place. The harness itself cannot be
bypassed accidentally — without `ARPG_HARNESS` there is no socket at all.
