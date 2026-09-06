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

---

## 4. SoA entity storage — *hooks*

The pass-decomposition half landed with the trace. Identity landed next:
`EntityId`, `World::spawn_enemy`/`despawn_enemy`, and the `slots` map that
gives a body a name its dense row cannot. Scenarios can now place a body at a
stated spot and assert about it by name — which was the binding constraint on
everything below, because a horde count puts N bodies in a grid whose positions
nobody wrote down.

**Gate — met.** `a_despawned_name_stays_dead` asserts a retired name stays dead
*after its slot is handed to somebody else*, and it is mutation-checked against
both the generation bump and the swap-remove fixup. The reuse half is
load-bearing: a first version stopped at the despawn and passed with the
generation bump deleted, because a vacated slot is caught by the vacancy
sentinel whether or not generations work.

### What is left in this chunk

The player joining the same storage, so passes take slices for it too rather
than its individual fields.

**This contradicts the argument written on `Player` itself**, which says it
stays a separate struct because wedging it into the horde would pay for
player-only fields — facing, attack phase, i-frames — N times. That argument was
correct against dense arrays and dissolves under the storage decision below:
player-only state lives in its own sparse set, so the player can be a body
without any enemy paying for what only it has. Resolve the two before starting,
rather than leaving a doc that argues against the change being made.

## Storage decision: sparse sets, not a dense table

A behaviour is **its own storage plus a pass that reads it**, not a field on a
god struct. An enemy "has" a behaviour when it is a member of that behaviour's
set; the pass iterates the set, so cost scales with membership rather than with
horde size, and adding a behaviour touches no existing type.

`slots` is deliberately payload-free for this reason — the same machinery sits
under the horde's positions and under a behaviour only three bodies have. The
alternative considered and rejected was one dense table with capability flags:
simpler and faster to write, but it makes every body pay for every behaviour and
turns the body table into the god class transposed into arrays.

## 4b. Contact as a query, separate from the response — *sim layer*

`pass::separate::pair` currently decides *that* two discs overlap and *what to
do about it* in one function. An attack wants the first half with a different
second half: deal damage, emit an event, do not push. Welded together, an attack
cannot reuse touching — it can only re-implement it.

Splitting it makes the overlap test a pure query returning a contact, with
separation as one *consumer* of that query and a hitbox as another.

**Gate:** a pure refactor, so the golden trace in `shoving_through_the_horde`
must come out byte-identical.

## 5. Uniform-grid spatial hash for broadphase — *sim layer*

Deferred with a number behind it. Brute-force crowd separation at the default
1024 bodies measures **0.217ms per tick** headless in release — about 1.3% of a
60Hz frame — so the grid buys nothing yet. It goes as the square, so 4096 is
~3.5ms and 8192 eats most of the frame; that is where it stops being an
optimisation and becomes the only way to raise N.

**Gate:** brute force and the grid produce identical contact sets over a
replayed input stream. A headless perf assertion enters the scenario runner
here: `step()` at 4096 enemies stays under budget, with no GPU and no window.

## 6. Offscreen capture — *perception*

Render to a target rather than the swapchain, so screenshots stop depending on
window visibility and become reproducible tick-for-tick. This is the whole of
the remaining gap in the perception stream.

**Gate:** the yaw pixel test and a `shot` scenario both pass with no window
present.

## 7. Separation steering — *sim layer*

Bodies currently collide only with the player, never with each other, and no
enemy has ever moved on its own. Brute force first, on top of chunk 4b's query:
the uniform grid in chunk 5 is an *optimisation of something already correct*,
and it can only be tested by agreeing with a correct thing that already exists.

**Both halves are done.** Crowd separation landed with `pass::separate::crowd`,
and `pass::seek` is the first behaviour only some bodies have.

**Gate — met.** `a_coincident_cluster_fans_out` resolves five bodies dropped on
one point, and `a_seeker_closes_and_a_bystander_does_not` asserts a chaser
closes 3.5 units in a second while a body without the behaviour never moves —
the bystander being the half that proves composition rather than motion.

### What is left

Bodies still spawn inert; `seekers <n>` over the harness is a debug dial, not a
spawner. A real one grants behaviours per body as it places them, from something
describing what *kind* of enemy this is. Deciding whether the default horde
chases is a game decision rather than an engine one, and it will churn three
golden traces when it is made — which is the right amount of ceremony for a
change that alters what the game *is*.

## 8. Attack state machine — startup / active / recovery, timed hitboxes

The payoff chunk, and the first where the machinery does work no unit test
could.

**Gate:** scenarios asserting the exact ticks a hitbox activates and
deactivates, and that a hit landing one tick outside the window does not
register.

## 9. Hitstop, knockback, input buffering

**Gate:** golden traces for impulse magnitude and hitstop duration, so a tuning
change produces a reviewable diff rather than a claim about feel.

---

## Known limitations (real, not yet worth fixing)

- **Nothing chases you.** Enemies have a position and a name and nothing else;
  they collide with the player but not with each other, and none has ever taken
  a step.
  Chunks 5 and 7 are what make the horde a horde.
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
