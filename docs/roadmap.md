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
  checkpoint and final-state assertions, exit 0 or 1. Every scenario is also replayed and hash-compared
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
- **Attack profile UI.** F1 opens a modal keyboard panel; arrows choose Basic,
  Thrust, Sweep, or Heavy sweep and R returns to Basic. The world keeps running.
  `AttackProfile` is authored game content; it resolves to a private-field
  `ResolvedAttack`, the same validated runtime value a future skill, weapon,
  stats, and buffs can produce through the atomic `try_new` boundary. The menu
  selects content rather than defining attacks as sequences of key increments.
  Settings are session-only and affect the next swing. The harness drives the
  same input route and reports selected and in-flight profiles beside their
  resolved values. `attack_profiles_apply_to_the_next_swing` asserts timing,
  geometry, knockback, and selection isolation against checkpoints and a golden
  trace. The earlier recovery-only path survives as `World::set_attack_recovery`,
  reachable from a scenario's `attack_recovery` field but *not* from the harness
  socket — it is a runtime modifier, not a command.

- **Scene playtests.** Authored scenes, exact disposable ownership, synchronous
  load/evict, and fresh restart through the F2 picker and harness. The game and gate share
  content decoding and instantiation. See [scene playtests](scene-playtests.md).

- **Health and defeat.** Three hits retire an enemy. Damage arrives through a
  sink that cannot reach body storage, and removal is the schedule's last pass,
  so the horde's rows hold still for every pass that reads them.

---

## 4. The player joins body storage — *hooks* — **done**

The player occupies the persistent first row of `Bodies`, with its own stable
identity. Facing and attack stay player-only. Physical membership and payload
use the same sparse store for the player and enemies. Resetting the horde
preserves the player's position, identity and carried motion.

**Gate:** existing positional scenarios stayed green through the refactor;
golden enemy IDs shifted by one because the player now owns a name.

## 5. Uniform-grid spatial hash for broadphase — *sim layer*

Deferred with numbers behind it. Brute-force crowd separation costs 0.14ms per
tick at 1024 bodies (0.8% of a frame), 2.1ms at 4096 (12%), and 32.6ms at 16384
— nearly twice the frame. So it buys nothing yet, and somewhere past 4096 it
becomes the only way to raise N. Micro-optimising will not help: from about 4096
the loop is memory-bound rather than arithmetic-bound.

**Gate:** brute force and the grid produce identical contact sets over a
replayed input stream, plus a headless perf assertion at 4096 enemies. The
machinery for the second half already exists — `physics_stays_within_tick_budget`
uses it — so this chunk only has to point it at a bigger horde.

## 6. Offscreen capture — *perception*

Render to a target rather than the swapchain, so screenshots stop depending on
window visibility and become reproducible tick-for-tick. This is the whole of
the remaining gap in the perception stream.

**Gate:** the yaw pixel test and a `shot` scenario both pass with no window
present.

## 7. A real spawner — *hooks* — **done**

Two halves, built in that order.

**The machinery under a spawner.** A bounded queue of requests, one pass
(`pass::spawn::drain`) that grants them, and a `Template` saying which
behaviours a new body is given. Spawning is structural — it moves rows other
passes hold indices into — so the drain is the only pass that changes what
exists, and it runs before anything holds a row.

**The thing that asks.** `pass::source::trigger` runs immediately before the
drain, is handed no storage, and may only push onto the queue: the code that
decides bodies exist cannot make one. A source is four independent axes —
cadence, `Condition`, `Placement`, `Template` — rather than one enum of every
combination, and it lives in its own list rather than on a body, because a
source is what *makes* bodies.

Two decisions worth knowing before changing anything here, both mutation-checked
against scenarios: a source starts **ready**, so its first body lands on the
first tick its gate is open rather than one cadence later; and a ready source
whose gate is shut **stays** ready, so enemies appear when the player walks in
rather than up to a cadence afterwards.

Whether the default horde chases is still a game decision, not an engine one,
and it will churn three golden traces when it is made.

**Gate:** met — `a_source_fires_on_the_ticks_it_promises` (the golden trace *is*
the tick list), `removing_a_source_stops_the_flow`, `a_source_waits_for_the_player_to_arrive`
(the tick predicted from `PLAYER_SPEED` before running),
`a_population_condition_fills_and_stops`, and
`two_kinds_of_enemy_from_one_description`.

### Left out on purpose

- **One body per fire.** Three at once is three sources or three ticks. A
  `count` axis is a field and a loop; it was cut because nothing needs it yet.
- **No budget.** A source runs until it is removed. `FewerThan` covers
  maintaining a population, which is what most uses of a count are reaching for.
- **Nothing in the simulation removes a source.** Still true, and no longer
  waiting on anything: see "A source is never destroyed by the game" under Known
  limitations for what actually blocks it.

## 8. Health, damage and death — *sim layer* — **done**

An enemy takes three hits. `pass::attack` subtracts one through a `DamageSink`
that cannot reach body storage, and `pass::health::remove_defeated` runs **last**
in the schedule, so the row-swapping a removal does cannot invalidate an index
an earlier pass is holding.

**Dense, not sparse — the prediction here was wrong and is worth recording.**
This chunk was planned as sparse membership plus a payload, following the
physical store. Health went dense instead; the reasoning is in the `pass/health.rs`
module doc, next to the storage it defends.

**Gate:** met — `three_hits_defeat_an_enemy` asserts health 2, then 1, then
`alive: false`, and its golden trace pins damage and removal to the hit ticks.
`despawning_preserves_a_swapped_survivors_payloads` covers the row swap with
unequal remaining health.

## 9. Hitstop, knockback, input buffering — **knockback done**

Knockback is an impulse into shared carried velocity. Solid contacts exchange
momentum without bounce; position projection does not create energy. Walls
cancel outward velocity and allow sliding. Walk and seek remain powered,
kinematic displacement, separate from carried velocity: they neither overwrite
knockback nor add locomotion speed to the conserved momentum calculation.

The attack applies six units of momentum along its facing, once per struck
body. A mass-one enemy travels about one unit in open space. Velocity changes
on the hit tick; displacement starts on the following tick. Sources other than
attacks use the same impulse interface. The harness exposes `impulse`, and state
reports each body's velocity and inverse mass by stable name.

**Knockback gate:** analytic impulse, mass, wall, damping and transfer scenarios;
`a_swing_pushes_a_body_it_did_not_hit` proves the attack/contact composition;
`physics_stays_within_tick_budget` measures headless simulation cost. Every
scenario still checks replay hashes, including physical state.

Hitstop and input buffering remain open.

**Gate:** golden traces for impulse magnitude and hitstop duration, so a tuning
change produces a reviewable diff rather than a claim about feel.

---

## Known limitations (real, not yet worth fixing)

- Physics uses discrete disc contacts and a single ordered solver sweep. The
  current six-unit knockback moves 0.1 units per tick; sufficiently large external
  impulses can tunnel through bodies. Swept collision detection is future work
  before introducing fast projectiles or much stronger launches.
- Momentum transfer covers carried velocity. Powered locomotion is kinematic;
  motor forces, steering suppression and frictional contact are separate work.
- **Death is removal, and nothing else.** A defeated body vanishes on the tick
  its third hit lands: no death animation, no corpse, no ragdoll, no drop, and
  no event anything downstream reacts to beyond `Event::Removed`. That is enough
  to assert the rule and not enough to read as a kill on screen.
- **Every chaser is identical.** One speed, one behaviour. A `Template` grants
  it per body and a source picks the template, so two kinds of enemy are
  describable; what is missing is a second thing for them to differ *in*.
- **`set_enemy_count` is a second spawn door, and it ignores templates.**
  `Bodies::respawn` grants default physics but no template behaviours and
  emits no `placed` events, where `place` and the drain do both. That is
  tolerable because it is a debug dial — `[`, `]` and `enemies <n>` — and not
  how the game will ever make a body. Unifying it means `set_enemy_count`
  taking a `Template`, which churns the scenario spec, the harness and a dozen
  tests to make a debug key more principled. Left alone deliberately; if the
  bulk path ever becomes gameplay, fix it then.
- **A source is never destroyed by the game.** Sources fire on a cadence and a
  condition, and only a scenario, the harness or a scene eviction removes one.
  No pass does. Bodies can die now, but `remove_enemy` retires the body, its
  behaviours and its scene ownership — it does not look for a source to retire
  with it, because a source is not hung off a body and deliberately so. A nest
  that dies when its body dies needs a source to *have* a body, which is the
  layering `pass/source.rs` argues against; the shape it actually wants is a
  `Condition` a defeat can close.
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
- Powered translation is instantaneous — full speed on the first tick, dead stop on
  release. Carried physical velocity is additional and damped. Responsiveness
  matters for the controls, and acceleration is a feel knob best tuned once combat exists. *Turning* is
  rate-limited; translation is not.
- Only the keyboard is wired. The action layer is what makes a gamepad or
  click-to-move additive: a second producer of `ActionMask`, nothing downstream
  touched.
- `cargo test --workspace` needs a real GPU adapter, because the headless
  render tests are not behind a feature flag. The scenario runner does not.
