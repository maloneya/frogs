# Agent principles

Why this project is shaped the way it is for agent work, and what "done" means
in each of the five streams. `CLAUDE.md` carries the short version, because it
is read on every edit; this file carries the reasoning and the open work.

The one-line thesis: **the bottleneck is not generating code, it is verifying
it.** Everything below exists to move a claim about the game from something an
agent asserts to something a process exits nonzero about.

---

## 1. Sim layer

`sim` is a pure function of (state, inputs). Given the same starting state and
the same ordered inputs, it produces byte-identical state every run.

The invariant binds the simulation — `World`, the fixed-timestep update,
physics, game logic. It deliberately does **not** bind presentation. Rendering,
interpolation, camera smoothing, particles and audio may do whatever reads
best, provided nothing they compute flows back into sim state. That exemption is
what lets the camera keep a half-life ease without costing reproducibility.

It is load-bearing rather than tidy: replay, save integrity, reproducible bug
reports, bisecting a regression to a tick, and every scenario in stream 4 all
rest on it.

**Where this stands. Done, and now checked rather than claimed.** `sim` links
neither wgpu nor winit, enforced by an allowlist that fails closed. There is no
RNG anywhere — `escape_direction` uses a golden angle specifically so two
identical runs cannot diverge. No wall clock inside `World`. No hash-ordered
iteration. And since chunk 1 the frame rate cannot reach the simulation at all:
`step` takes a `Dt` that carries no number and can only have come from the
accumulator, so a variable timestep is unrepresentable rather than merely
discouraged.

`World::hash()` covers every field, enforced by exhaustive destructuring — a new
field is a compile error until it is hashed. Every scenario is replayed and
compared tick by tick, so a divergence reports *which* tick rather than merely
that one happened.

**Open work.** One known landmine, left deliberately with a comment rather than
guessed at: input is sampled once per frame but a frame can run several ticks,
so the first *edge-triggered* action — an attack button — would fire once per
tick instead of once. That is the input-buffering problem and it belongs to the
chunk that adds the first such action. Seeded randomness, if it is ever wanted,
takes a generator from the world with the seed recorded alongside the input
stream; there is no RNG today, so there is nothing to convert yet.

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
version is enough.

**Where this stands.** Excellent at the crate boundary, absent below it, and
the gap has widened. `gfx` cannot name a sim type, `sim` cannot name a key,
`scenario` cannot name either a renderer or a window — all enforced at layer 0
or 1 by allowlists that fail closed.

Inside `sim` it is still one file with a single `step()` doing everything in an
order held only in control flow, and that order is now genuinely load-bearing:
`remember()` must run before anything moves or every body on screen streaks. So
the `add-sim-pass` skill still cannot be followed — it says "register your pass
in the schedule", and there is no schedule.

One thing did change in this stream's favour: a behaviour-preserving pass split
is now *safe*, because the per-tick hash sequence is a gate that proves a
refactor changed nothing. Doing it before three combat systems grow into
`step()` is the cheap moment, and that moment is now.

**Open work**, best done in the same chunk as SoA storage, because SoA is what
makes disjoint slices exist:

- `step` becomes an explicit ordered list of named pass functions rather than
  inline stages, so the ordering is data you can read and assert on.
- Each pass takes the slices it needs rather than `&mut World`. The borrow
  checker then enforces "a pass touches only what it declares" at layer 0, free.
- One module per pass, with that pass's constants and const asserts beside it.

Doing this at the SoA chunk costs almost nothing. Doing it after three combat
systems have grown into `step()` costs a rewrite.

---

## 3. Agent perception

Agents generate code competently and cannot see what they built. Headless
solves speed and reach; perception is a separate and harder problem. You can be
fully headless and still blind.

Four things count as perception: structured world queries, a tick-stamped event
trace, deterministic screenshots, and eventually causal introspection ("why did
entity 93 lose 4 hp" answered with a system, a tick, and the components read).

**Where this stands. The weakest of the five, and now clearly so.** Streams 1
and 4 moved a long way; this one gained a `tick` field in `state` — which is
itself an instance of the problem, since rule 3 says not to hand-maintain that
string.

The harness is a first-rate *control* surface and a thin *perception* one, and
the asymmetry is the point. Control has `press`,
`release`, `tap`, `hold`, `wait`, `enemies`, `vsync`, all with reply-means-landed
semantics. Perception has one line of space-separated numbers and a PNG. That
line is a hand-maintained format string, which by this project's own reasoning
is a second table to forget. It is also a point sample: nothing that happened
*between* two `state` calls survives.

**Open work, in order.**

- **Trace stream.** Tick-stamped typed events in a ring buffer, `trace since
  <tick>` on the socket, written to file by scenarios. Build this *before* the
  first hitbox, not after. Attack windows, hitstop and knockback fail silently —
  no crash, no compiler error — and a snapshot taken afterward cannot see a
  three-frame timing error inside a twelve-frame window. Absence of explicit
  trace signals is what makes that class of bug hardest to catch.
- **Derive `state`** from the data and emit JSON, so adding a field to the world
  makes it observable without a second edit.
- **Offscreen capture.** `shot` currently rides on the presented surface, so it
  is coupled to window visibility. Headless wgpu already works in the yaw pixel
  test. Rendering to an offscreen target at a named tick makes screenshots work
  occluded, work with no window, work in CI, and be reproducible tick-for-tick —
  which is the precondition for any judgement about an image meaning anything.

---

## 4. Scenario protocol

A scenario is the unit of verification: setup, an input stream in ticks,
assertions over final state and over the trace, a tick budget. It runs headless
against `sim` with no GPU and no window, and it exits 0 or 1.

Two things follow. Verification stops being a judgement call. And a prediction
becomes durable — today the predict-then-measure ritual in the `playtest` skill
produces a result that is discarded the moment it passes.

**Where this stands. Built.** `crates/scenario` runs `.ron` files headlessly
against `sim` and exits 0 or 1; the `Stop` hook runs it when a turn ends and
blocks on failure. A scenario is setup, tick-indexed input spans, a tick budget,
and assertions over final state.

Two properties are worth more than the file format. Every scenario is replayed
and compared by per-tick hash **whether or not it asks**, so determinism is
checked by every scenario written for any other reason. And
`deny_unknown_fields` means a scenario asserting something the runner does not
implement is *refused* rather than silently ignored — an ignored assertion is
the worst possible outcome, because it reads exactly like a passing one.

The runner was verified the way everything else here is: by breaking the
simulation on purpose and checking it noticed. A speed constant drifting by
0.5%, a wall clamp off by a body radius, a wall clock inside `step`, and an
off-by-one in the runner's own input scheduling were all caught with a message
naming the cause.

**Open work.** Trace assertions and golden traces, which arrive with the trace
itself. Setup primitives beyond the enemy count — placing the player, spawning a
body somewhere specific — which want `World::spawn` and land with SoA storage.
GPU tests behind `#[cfg(feature = "gpu")]` so `cargo test --workspace` is green
without an adapter, which it still is not; the scenario runner already is.

Speed is part of the design, not a bonus. `hold d 500` costs 500ms of wall
clock; the same thirty ticks against `sim` cost microseconds. That is the
difference between a loop running five checks a minute and five thousand.

---

## 5. Debug protocol

A file keyed by *observable symptom*, appended to when a failure is diagnosed,
consulted before guessing. Each entry is signature → cause → check → fix.

Two properties make it worth more than the same text as prose. It is indexed by
what you *see*, so an agent staring at a black PNG finds the entry without
having read everything. And it accumulates without growing the file that is
loaded at the start of every session.

It has a second half that is easy to miss: entries generalise into
*pre-execution* checks. Once a class recurs, it stops being a lookup and becomes
a validation that runs before the work. Most of the measured benefit comes from
that half.

**Where this stands.** The content is excellent and the form is wrong. "12,467
fps with vsync on, entirely skipped frames." "Backgrounded presents honestly but
throttled, `skipped` at zero." "Synthetic keystrokes go to whatever *is*
focused." Every one is a real signature/cause/fix triple bought with hours, and
every one is stored as narrative, indexed by topic, split across three sections
of two files.

**Open work.** `docs/traps.md` exists now, seeded with those three. The rule
that makes it more than a notes file is promotion: an entry sits at layer 4, the
weakest tier in the ladder, and an entry that fires three times is telling you it
belongs at layer 0–3. Dated entries make that queue visible instead of dependent
on memory.

---

## The gate that ties them together

Every claim an agent makes about its own work has to bottom out in a process
that exits nonzero. The clippy `PostToolUse` hook is one. The scenario runner is
one. A headless perf assertion is one. A golden trace diff is one.

Anywhere a claim does not bottom out that way, the loop quietly degrades into
the agent grading itself — and that reads, from the inside, exactly like
success.
