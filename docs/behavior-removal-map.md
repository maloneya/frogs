This historical worklist records the pre-refactor owners. See [actor behaviors](behaviors.md)
for the replacement and its current owner links. Paths marked removed below are historical.

This is the removal map for replacing the current pursuit/attack systems with
composable entity behaviours. It records the current checkout, including the
uncommitted swipe/leap work. It is an implementation worklist, not a second
schema or a promise to preserve obsolete APIs. No runtime changes belong to
this mapping step.

The clean cut is to remove competing decision and action owners, while retaining
the handwritten mechanisms underneath them. Moving the existing enemy controller
behind a new interface would leave its player-only targeting, seeker requirement,
and explicit-body-only attack grants intact.

| Removal or replacement | Current owner | What survives / replaces its responsibility |
| --- | --- | --- |
| Sim-owned seeker policy, membership, and its public grants/queries | `crates/sim/src/pass/seek.rs` (removed), [World](../crates/sim/src/lib.rs), [Game forwards](../crates/game/src/game.rs) | Game-owned behaviour chooses movement; sim accepts a validated physical request. |
| Identity/time gait lottery and global monster-specific movement policy | `crates/sim/src/pass/seek.rs` (removed), [tuning](../crates/sim/src/tuning.rs) | Behaviour/action-owned movement settings and intent; preserve bounded acceleration and speed validation. |
| Seeker override command path | `SeekerDrive` in `crates/sim/src/pass/seek.rs` (removed) | One shared motor request boundary with explicit control ownership, usable by any eligible actor. |
| Combined enemy decision, action, coordination and health controller | `crates/game/src/enemy_attack.rs` (removed) | Separate behaviour selection, action execution, coordination policy and shared damage/health authority. |
| Explicit-body attack side list plus physical `seeks` flag | [GameScene](../crates/game/src/scene.rs), [Template](../crates/sim/src/pass/spawn.rs) | One game-owned actor/behaviour definition path across placed, generated and queued births. |
| Seeker-gated attacker/player geometry | `MeleeView` in [access.rs](../crates/sim/src/access.rs) | Restricted physical observations using explicit actor and target identities. |
| Implicit movable-body = enemy = damageable classification | [body admission, population and presentation](../crates/sim/src/lib.rs), [health](../crates/sim/src/pass/health.rs) | Separate physical capability, health, target eligibility and population classification. |
| Actor-hardcoded attack/interaction initiation and separate player damage path | [player combat](../crates/sim/src/pass/attack.rs), [interaction](../crates/sim/src/pass/interact.rs), the removed enemy controller | Explicit actor/target actions and common effect mechanisms; retain player controls and existing abilities. |
| Old seeker/gait/attack-specific observation contracts and consumer dependencies | [trace](../crates/sim/src/trace.rs), [scenario vocabulary](../crates/scenario/src/spec.rs), [presentation](../crates/app/src/presentation.rs), [effects](../crates/app/src/attack_effects.rs) | Read-only observations of authoritative definitions, running actions and outcomes; presentation stays downstream. |

The removals have the following concrete boundaries.

1. **Remove the seeker as a behaviour owner.** Delete the `seek` pass and
   `World.seekers`, not the reusable `Members` storage type. Remove
   `add_seek`, `is_seeker`, `seeker_count`, `gait`, their Game forwards, their
   report/hash participation, and their plumbing through removal, scene eviction,
   spawn, health cleanup and encounter access. Remove `Template.seeks`,
   `Template::seeking`, `Template::seeks`, and the seeker-specific validation and
   grant in spawn. The new behaviour must not require a legacy seeker underneath
   it. Direct pursuit remains available as a behaviour assembled from movement.

2. **Remove the independently scheduled gait decision.** Retire `Gait::sample`,
   its identity/tick selection, `GaitsChanged`, corresponding `EventKind` and
   scenario event matching, and reports claiming those choices as current intent.
   Walk/run/sprint can remain useful movement settings; their selection belongs
   to the behaviour. Replace the `MonsterWalk`, `MonsterRun`, `MonsterSprint`,
   `MonsterAcceleration` policy ownership and `Tuning::gait_speed` coupling
   together with tuning UI, persistence and assertions. This does not remove
   acceleration/braking mathematics or necessarily discard the existing speeds.

3. **Replace the whole enemy controller, including the new prototype.** Retire
   `EnemyAttackSpec`, `Attacker`, `EnemyAttacks`, `EnemyAttackStatus`,
   `EnemyAttackKind` as the current standalone controller API, and its
   `prepare`/`advance` schedule. Swipe and leap remain gameplay actions, but
   the distance switch, committed action state, cooldowns, hit tracking and
   coordination no longer live in one enemy-only store. Replace its Game fields,
   exports, scene error, admission/install, restart snapshot, hash/report,
   despawn and eviction handling in the same cutover. Move `player_health` out
   of this controller; enemy damage must no longer directly decrement a second
   health implementation. Combat telemetry such as `PlayerDamaged` must not
   remain conceptually owned by [encounter events](../crates/game/src/encounter.rs).

4. **Remove override-based movement authority.** `SeekerDrive` currently works
   only because the seeker pass checks it; non-seekers are silently skipped and
   the first matching override wins. Replace this dependency with validated
   generic motor requests after action control has been resolved. Preserve
   [DriveSink](../crates/sim/src/pass/motion.rs): one motor request per body per
   tick, drag applied once, bounded acceleration, immovable-body checks, and
   composition with impulses and contacts. An action cannot overwrite physical
   velocity to make its plan come true. Avoid a new per-actor scan of every
   actor's requests; the current small override list is not a scalable general
   action interface.

5. **Remove implicit actor/target assumptions.** `MeleeView::positions` currently
   requires seeker membership and always supplies player row zero as the target.
   Player attacks are offered all non-player bodies, and interaction is initiated
   from player position. Actor identity, target identity/selection and eligibility
   must become explicit at their owners. Preserve the validated attack geometry,
   committed hitbox samples, per-target hit deduplication, wall queries, damage
   and impulse sinks, and one-shot interaction endpoint state. Player input is
   still a distinct producer; it should not require its own incompatible action
   lifecycle. Preserve Cleave, Slam, Disintegrate, aiming and movement feel while
   removing actor-hardcoded execution paths. There is no need to rebuild every
   ability's geometry or replace the physical player row-zero storage convention
   simply to make action inputs explicit.

6. **Remove health membership as an enemy classifier.** Currently every movable
   non-player body is automatically granted ordinary enemy health. That same
   membership controls `enemy_count`, source population gates, player attack
   eligibility, enemy presentation and the inverse prop presentation filter.
   A civilian would therefore become an enemy in several systems even without
   an aggressive behaviour. Health, hostility/target eligibility, render role,
   and the population counted by source gates need explicit owners. Generalize the shared
   health/effect mechanism instead of creating an NPC-specific damage path.
   Preserve source attribution and authoritative defeat outcomes. Decide defeat
   handling separately from health membership: admitting the player to shared
   health must not accidentally despawn protected row zero or change the current
   zero-health run policy.

7. **Remove the split authoring model everywhere.** `GameScene.enemy_attacks`
   binds only explicit bodies; enabling the editor checkbox also forces `seeks`.
   Delete this side-list contract and the checkbox/forced grant together. Update
   [headless authoring](../crates/authoring/src/editor.rs) and its native consumer,
   component duplication/deletion, reference remapping, validation, saved drafts
   and scene persistence. The replacement definition must be available to
   explicit placements, formations, source emissions, immediate placement,
   queued requests, scenarios and harness spawns. Preserve the shared source
   cadence and queue mechanisms; change what gameplay data is attached to their
   births. A scene-to-body side list with a new name would not solve this.
   Rewrite repository content directly and reject obsolete fields—no aliases,
   legacy readers or defaults that silently translate old behaviour. Update
   [playable scenes](../scenes/), including the 1,000-enemy trial, separately from
   test-owned [scenarios](../scenarios/) and [benchmark workloads](../benchmarks/).

8. **Remove consumer dependencies on the old decisions, not useful presentation.**
   The app currently branches on enemy attack kind/phase for tilt, leap height
   and markers. It also takes a fixed array of at most `MAX_ENEMY_COMMITMENTS`
   statuses, and sizes effect geometry using that combat policy. Replace these
   feeds with bounded action presentation facts. A combat pressure limit must
   not cap the number of actors allowed to move, wait, interact, or animate.
   Keep actual-displacement-based locomotion roles, shared bounded crowd pose
   buckets, interpolation, read-only feedback, audio deduplication and render
   budgets. Remove their dependency on sim `Gait` reference speeds; do not delete
   the movement animation roles just because their names resemble gaits. Update
   health HUD/audio reads and enemy/prop presentation classification alongside
   the new health and role owners. Ground travel plus a visual leap arc remains
   the current leap mechanic; this refactor does not imply airborne immunity or
   jumping over solid walls.

9. **Replace old tests and diagnostics by purpose.** Remove scenario `Seek`,
   `seeking`, `seekers`, `gait`, old gait events and obsolete tuning/status
   expectations. Retire assertions of the exact hash-derived gait schedule in
   [horde_motion.rs](../crates/game/tests/horde_motion.rs) and
   `chaser_gaits_accelerate_independently.ron` (replaced by [authored speed predictions](../scenarios/actor_speeds_accelerate_independently.ron)).
   Preserve their independent guarantees: deterministic decisions, stale-ID
   refusal, row-swap stability, acceleration rather than velocity replacement,
   restart, eviction and diagnostic independence. Rewrite [attack](../crates/game/tests/enemy_attack.rs)
   and [leap](../crates/game/tests/enemy_leap.rs) tests against new action owners,
   keeping their predicted timing, misses, single-hit windows and interactions.
   Preserve pushing, braking, wall, source cadence, queue and broad-phase tests
   that happen to use seeker fixtures. Narrow [Game/World equivalence tests](../crates/game/tests/engine_compatibility.rs)
   to shared mechanisms: Game-owned decisions should not also run inside World
   merely to keep an old equivalence assertion passing. Update harness examples,
   authoring tests, native companion fixtures, reports, docs and relevant task
   guides with the actual final vocabulary.

Several scheduling contracts must be settled before implementation is split
among agents:

- Source triggering and queued spawn draining currently happen inside the sim
  tick, before seeking. Newborn seekers act on their birth tick. Preparing all
  new behaviours before the existing World step would accidentally delay them.
  Establish a structural-admission boundary that attaches complete actor state
  before decisions, retaining source ownership, FIFO outcomes, capacity refusal
  and accepted queued work. Do not recover births by reading diagnostic traces.
- Enemy attack decisions currently read prior completed positions; seeking reads
  positions after player walking. State explicitly which facts each new pass
  observes, then predict intermediate ticks. Do not conceal an ordering change
  behind rewritten goldens.
- Existing player attacks resolve and remove defeated enemies before enemy
  damage resolves. This prevents defeated enemies trading late hits. Preserve
  that gameplay rule or deliberately specify and test a replacement; a unified
  action runner must not accidentally reverse the priority.
- [DamageSink::hit](../crates/sim/src/pass/health.rs) currently asserts that a
  target still has positive health. Several action producers can hit the same
  target before final structural removal. Replace that single-producer
  assumption with explicit deterministic damage/death admission, including
  subsequent hits after lethal damage. Preserve one authoritative defeat and
  the final removal boundary; test multiple actors hitting one target in a tick.
- The new action contract needs explicit requirements, committed state, outcomes,
  cancellation and control ownership. Targets can become stale, scenes can be
  evicted, and actions can be interrupted. Retirement must release both control
  and any coordination reservation exactly once.
- Shared behaviour definitions and per-entity running state are different
  lifetimes. Validate complete definitions before mutation, and hash/report all
  authoritative runtime state. Derived commands and presentation remain outputs.
- Global attack admission is currently entangled with execution and rendering.
  Separate it as a coordination policy and explicitly choose its reservation
  lifetime. Retaining the current pressure settings initially is possible;
  retaining their ownership in the enemy controller is not.

The work should preserve the handwritten loop, stable IDs, dense/sparse storage,
spatial partition, physics/contact solvers, wall geometry, authoritative spawn and
defeat outcomes, encounter/door/chest mechanisms, and bounded renderer. A general
ECS, string-based scripting language, editor for arbitrary behaviour trees,
comprehensive faction simulator, or new pathfinder is not necessary to replace
these owners. Likewise, shared action lifecycle does not mean allocating the
player's maximum-size melee buffers on every NPC.

A coherent implementation order is:

1. Specify the replacement ownership, actor/capability admission, narrow action
   contracts and tick schedule, including spawn and defeat boundaries above.
2. Cut over definitions and all birth paths to one validated game-owned actor
   configuration; replace implicit enemy/health/target classification.
3. Establish the shared action execution and physical effect boundaries. Port
   movement, waiting and interaction to prove a noncombat NPC; port pursuit,
   swipe and leap using the same lifecycle and control rules. Preserve existing
   player abilities as explicit producers of shared effects/actions.
4. Convert both authoring clients, content, observation consumers and independent
   test setups; delete old entry points and schemas in that cutover. Temporary
   local edits may be incomplete, but the delivered system must not run two
   behaviour implementations or retain a compatibility mode.
5. Review for leftover ownership and vocabulary, then verify correctness and
   performance against the new observable contracts.

Completion requires more than the old scenes loading. A monster and noncombat
NPC must share movement/action machinery while using different definitions;
several entities must share a definition without sharing mutable progress;
actors with different action sets must reject unavailable actions before
mutation; civilians must neither become hostile targets by default nor close
enemy population gates. The same definition must reach direct, grid, source and
queued births. Tests must cover stale targets, interruption, cancellation,
removal/eviction/restart, hit timing, defeat priority, replay and report/hash
coverage through Game. Large moving/attacking populations must retain bounded
rendering and suitable per-tick work.

Follow the [verification policy](verification.md): focused predictions while
editing, then `python3 scripts/verify.py full --snapshot`, and explicit live
performance coverage for rendering/frame-pacing claims. Keep scenario and
benchmark setups independent of playable files. Rewrite moving/compressed
benchmark definitions deliberately and remeasure; never widen a performance
gate or bless changed evidence to make the refactor pass. Historical evidence
remains historical evidence. This map itself adds no runtime coverage claims.
