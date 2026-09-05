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

Inside `sim`, **the schedule now exists.** `step` is an ordered list of calls
into `pass/` and contains no logic of its own; `pass/mod.rs` documents the order
and why each adjacency is what it is; each pass owns its tuning constants, their
const asserts and its tests. A pass takes the data it declares rather than
`&mut World`, so for the horde "this pass touches only positions" is enforced by
the borrow checker at layer 0.

It was done at the cheap moment and not later, and it was only *safe* to do
because chunk 1's per-tick hash makes a behaviour-preserving refactor
checkable — six scenarios passing unchanged is what proved the split changed
nothing. That is the two streams paying each other back.

**The remaining gap is the player.** It is a single struct rather than a row in
the horde's storage, so passes take its individual fields where the horde gets a
real slice: the horde half of every signature is checked by the compiler, the
player half by reading it. Both become slices when the player joins SoA storage
and `EntityId` exists.

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

**Where this stands. Half solved.** The trace exists: tick-stamped events in a
ring buffer, `trace since <tick>` on the socket, and golden trace files that a
scenario compares against, so a timing change is a reviewable diff rather than a
claim. `TraceSink` is bound to the tick being run, so a pass cannot stamp an
event with the wrong one — the single error that would have made every timing
assertion built on it worthless.

It earned its keep immediately: the first golden file it produced is missing
tick 3, because the player bounces clear of the crowd for exactly one tick.
Nothing in final state could show that.

What has *not* moved is the structured query. `state` is still a hand-maintained
format string, and it gained a `tick` field this session — which is itself an
instance of the problem rule 3 names. Deterministic screenshots have not moved
either. So the harness is still a first-rate *control* surface and a
half-finished *perception* one, and the asymmetry is the point. Control has `press`,
`release`, `tap`, `hold`, `wait`, `enemies`, `vsync`, all with reply-means-landed
semantics. Perception has one line of space-separated numbers and a PNG. That
line is a hand-maintained format string, which by this project's own reasoning
is a second table to forget. It is also a point sample: nothing that happened
*between* two `state` calls survives.

**Open work, in order.**

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
