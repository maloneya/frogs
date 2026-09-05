# Agent principles

**Why** this project is shaped the way it is for agent work. `CLAUDE.md` carries
the rules that fire while editing; `docs/roadmap.md` carries what is built and
what is next. This file is the reasoning, and deliberately holds **no status** —
a second place recording what exists is a second place to forget, which is the
failure these five streams exist to prevent.

The one-line thesis: **the bottleneck is not generating code, it is verifying
it.** Everything below exists to move a claim about the game from something an
agent asserts to something a process exits nonzero about.

---

## 1. Sim layer

`sim` is a pure function of (state, inputs). Given the same starting state and
the same ordered inputs, it produces byte-identical state every run.

The invariant binds the **simulation** — `World`, the fixed-timestep update,
physics, game logic. It deliberately does **not** bind presentation. Rendering,
interpolation, camera smoothing, particles and audio may do whatever reads best,
provided nothing they compute flows back into sim state. That exemption is what
lets the camera keep a half-life ease without costing reproducibility.

It is load-bearing rather than tidy: replay, save integrity, reproducible bug
reports, bisecting a regression to a tick, and every scenario in stream 4 rest
on it.

**Scope, written down before someone assumes otherwise:** f32 determinism holds
for one binary on one machine. Given the macOS/M4-only stance that is enough for
replay, scenarios and debugging. It is *not* enough for lockstep netcode, and
promising that would require fixed-point maths.

---

## 2. Hooks and plugins

An agent adding a behaviour should never write lifecycle, wiring or ordering
code. It should fill a designated extension point. This is not a preference:
where it has been measured, forcing an agent to write from scratch rather than
override a hook was the single largest degradation of any workflow constraint
tested, costing roughly ten points of build health and twelve of intent
alignment.

**This does not mean building a plugin framework.** Indirection for its own sake
fights the first-principles goal and buys nothing at this size. The Rust-native
version is enough: a named pass, taking the data it declares, registered in an
ordered schedule. The borrow checker then enforces "a pass touches only what it
declares" for free, and the ordering becomes something you can read rather than
something held in control flow.

---

## 3. Agent perception

Agents generate code competently and cannot see what they built. Headless solves
speed and reach; perception is a separate and harder problem. **You can be fully
headless and still blind.**

Four things count as perception:

- **Structured world queries** — derived from the data, never a hand-maintained
  format string. A second list of the world's fields is a list that drifts.
- **A tick-stamped event trace.** State is a point sample and cannot see an
  interval. Attack windows, hitstop and knockback fail with no crash, no
  compiler error and no failing unit test; the mistake exists only between two
  samples.
- **Deterministic screenshots**, reproducible tick-for-tick and independent of
  whether a window happens to be visible.
- **Causal introspection**, eventually — "why did entity 93 lose 4 hp", answered
  with a system, a tick, and the components read.

---

## 4. Scenario protocol

A scenario is the unit of verification: setup, an input stream measured in
ticks, assertions over final state and over the trace, a tick budget. It runs
headless against `sim` with no GPU and no window, and it exits 0 or 1.

Two things follow. Verification stops being a judgement call. And a prediction
becomes durable — the predict-then-measure ritual in the `playtest` skill
otherwise produces a result discarded the moment it passes.

Speed is part of the design, not a bonus. `hold d 500` costs 500ms of wall
clock; the same thirty ticks against `sim` cost microseconds. That is the
difference between a loop running five checks a minute and five thousand.

---

## 5. Debug protocol

A file keyed by **observable symptom**, appended to when a failure is diagnosed,
consulted before guessing. Each entry is signature, cause, check, fix.

Two properties make it worth more than the same text as prose. It is indexed by
what you *see*, so an agent staring at a black PNG finds the entry without
having read everything. And it accumulates without growing the file loaded at
the start of every session.

It has a second half that is easy to miss: entries **generalise into
pre-execution checks**. An entry sits at layer 4, the weakest tier in the
ladder, and one that fires three times is telling you it belongs at layer 0-3.
Promote it, then delete the entry and record the promotion. Most of the measured
benefit comes from that half; a trap file that only ever grows is one nobody
reads.

---

## The gate that ties them together

Every claim an agent makes about its own work has to bottom out in a process
that exits nonzero. The clippy `PostToolUse` hook is one. The scenario runner is
one. A headless perf assertion is one. A golden trace diff is one.

Anywhere a claim does not bottom out that way, the loop quietly degrades into
the agent grading itself — and that reads, from the inside, exactly like
success.
