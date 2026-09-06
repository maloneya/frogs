# Roadmap

What is not built yet, and the **gate** for each — the thing that must exit 0
before the chunk is done. "It looks right when I run it" is not a gate.

One chunk at a time, built and running before the next starts.

## Built so far

Ordered as they landed. The reasoning behind each is in its commit message;
what matters here is what it left behind to build on.

- **Ortho camera, ground grid, instanced cubes** — one draw call for the whole
  horde, runtime-adjustable N.
- **Input → actions → a character** that moves and faces where it walks, with a
  camera that follows and leads it.
- **Circle colliders** — the player displaces the horde rather than passing
  through it.
- **Fixed timestep.** `Dt` carries no number and only `Accumulator` mints one,
  so the frame rate cannot reach the simulation. `World::hash` covers every
  field, enforced by exhaustive destructuring.
- **Render interpolation.** `Alpha` blends the last two ticks inside `extract`,
  which takes `&self` — so interpolation cannot reach sim state.
- **Scenario runner.** RON setup, tick-indexed inputs, a tick budget,
  assertions, exit 0 or 1. Every scenario is also replayed and hash-compared
  whether or not it asks. The `Stop` hook runs it.
- **Trace, and the pass schedule.** Tick-stamped events in a ring buffer;
  `TraceSink` is bound to its tick, so a pass cannot misdate an event. `step` is
  an ordered list of calls into `pass/`, each taking the data it declares.
- **Derived `state`.** JSON from `World::report`, which destructures `World`
  exhaustively — a field added to the world will not compile until it is
  observable.
- **Entity identity.** `EntityId` is a slot plus a generation, so a retired name
  resolves to nothing rather than to whoever inherited its row. Scenarios place
  bodies at stated spots and assert about them by name.
- **Contact as a query.** `contact::between` says whether two discs touch and
  along what line, and stops there. Separation and the hitbox are two responses
  to one question.
- **The horde is a crowd.** Bodies separate from each other as well as from the
  player, brute force.
- **Behaviours as sparse sets.** A behaviour is its own membership plus a pass
  that walks it, so adding one touches no existing type. `pass::seek` is the
  first; see `crates/sim/src/members.rs` for why the storage is shaped this way.
- **The swing.** Startup, active and recovery with a timed hitbox, asserted tick
  by tick against golden traces.

---

## 4. The player joins body storage — *hooks*

Identity landed; what is left is the player becoming a row like everything else,
so passes take slices for it too rather than its individual fields.

**Resolve a contradiction first.** The doc comment on `Player` argues it stays a
separate struct because folding it into the horde would pay for player-only
fields N times. That was correct against dense arrays and dissolves under sparse
sets: player-only state lives in its own membership set, so the player can be a
body without any enemy paying for what only it has. Fix the comment or abandon
the chunk — do not leave a doc arguing against the change being made.

**Gate:** scenarios stay green across the refactor.

## 5. Uniform-grid spatial hash for broadphase — *sim layer*

Deferred with numbers behind it. Brute-force crowd separation costs 0.14ms per
tick at 1024 bodies (0.8% of a frame), 2.1ms at 4096 (12%), and 32.6ms at 16384
— nearly twice the frame. So it buys nothing yet, and somewhere past 4096 it
becomes the only way to raise N. Micro-optimising will not help: from about 4096
the loop is memory-bound rather than arithmetic-bound.

**Gate:** brute force and the grid produce identical contact sets over a
replayed input stream. A headless perf assertion enters the scenario runner
here: `step()` at 4096 enemies stays under budget, with no GPU and no window.

## 6. Offscreen capture — *perception*

Render to a target rather than the swapchain, so screenshots stop depending on
window visibility and become reproducible tick-for-tick. This is the whole of
the remaining gap in the perception stream.

**Gate:** the yaw pixel test and a `shot` scenario both pass with no window
present.

## 7. A real spawner — *hooks*

Bodies spawn inert and `seekers <n>` is a debug dial, not a spawner: "the first
n" is a fact about storage order rather than about the game. A real one grants
behaviours per body as it places them, from something describing what *kind* of
enemy this is.

Whether the default horde chases is a game decision, not an engine one, and it
will churn three golden traces when it is made.

**Gate:** a scenario places two kinds of enemy from one description and asserts
they behave differently.

## 8. Health, damage and death — *sim layer*

A hit currently does nothing but say so. The seam is already the right shape:
`pass::attack::strike` records who it touched, so damage is a change to that one
function.

Health is also the behaviour that exercises the *payload* half of a sparse set,
which `seek` did not need — membership plus a parallel array, kept in step
through the row `Members::add` hands back.

**Gate:** a scenario kills a body with a known number of swings and asserts its
name goes dead; a golden trace shows the tick it died on.

## 9. Hitstop, knockback, input buffering

**Gate:** golden traces for impulse magnitude and hitstop duration, so a tuning
change produces a reviewable diff rather than a claim about feel.

---

## Known limitations (real, not yet worth fixing)

- **Nothing dies.** A swing registers hits and that is all it does — no health,
  no damage, no corpses. Chunk 8.
- **Every chaser is identical.** One speed, one behaviour, granted in bulk by a
  debug dial. Chunk 7.
- `World::extract()` rebuilds the 16384 static ground tiles every frame and
  re-uploads the whole instance buffer. Deferred with a number behind it: 17409
  instances render in ~3ms uncapped on the M4, so a static/dynamic split is not
  yet buying anything.
- `Clock` smooths frame time with an EMA, which *hides* pacing variance. An
  average is the wrong instrument for the thing that matters most here; a
  frame-time histogram is the intended replacement.
- The camera angle is fixed. That is also what lets `ground_basis` be the only
  screen/world translation without a feedback loop between input and view.
- The camera does not clamp to world bounds, so walking to the very edge shows
  the void. Measured rather than guessed: the view covers ~57x55 units of floor,
  so on the old 48-unit arena a bounds-clamped camera could have moved ±8 units
  total — pinned before the player reached the edge. Real level geometry is the
  eventual fix; enlarging the world was the interim one.
- No input deadzone. Deliberate: it trades micro-jitter for a sticky region and
  a snap at its boundary, and against a horde the smoothed follow reads better.
- Translation is instantaneous — full speed on the first tick, dead stop on
  release. Deliberate: responsiveness beats momentum in an ARPG, and
  acceleration is a feel knob best tuned once combat exists. *Turning* is
  rate-limited; translation is not.
- Only the keyboard is wired. The action layer is what makes a gamepad or
  click-to-move additive: a second producer of `ActionMask`, nothing downstream
  touched.
- `cargo test --workspace` needs a real GPU adapter, because the headless
  render tests are not behind a feature flag. The scenario runner does not.
