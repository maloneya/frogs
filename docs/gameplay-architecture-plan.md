# Gameplay architecture and source-control plan

Status: all six phases complete. This document records the architecture
and staged migration discussed on 2026-09-10, with lessons recorded after
implementation on 2026-09-11. The architecture sections describe the ongoing
ownership rules; the checklist records the migration and its verification.
Follow-up work remains separate from the completed six phases.

## Purpose

Build a scenario where interacting with an entity controls a spawner, while
establishing a home for gameplay relationships outside the simulation engine.
The first proposed playable slice is a dormant source started by one-shot
activation. Shutdown and repeatable toggling are separate behaviour choices;
repeatable interaction is not part of this first slice.

The architecture should reduce how much code an agent must understand for a
local change and make incorrect changes fail loudly. Crates are both a map of
responsibility and enforceable dependency boundaries. Moving files without
restricting access would not achieve that goal.

## Target responsibilities and dependencies

| Crate | Owns |
|---|---|
| `core` | Small vocabulary needed across independent layers; no gameplay relationship definitions |
| `sim` | Physical world, identity, capabilities, spawning, and validated operations protecting shared simulation mechanisms |
| `game` | Complete playable state, gameplay relationships and mechanics, and tick orchestration |
| `content` | Shared decoding of authored files into definitions supplied by their owning crates |
| `gfx` | Rendering mechanisms, independent of gameplay meaning |
| `app` | Window, devices, presentation, and the live harness |
| `scenario` | Headless driving and assertions over the same game the app runs |

Principal dependencies (an arrow means "depends on"):

```text
app      -> game, content, gfx
scenario -> game, content
content  -> game, sim
game     -> sim
sim      -> core
gfx      -> core
```

This is the responsibility graph, not a complete list of imports: adapters may
also need shared value types. Maintain explicit dependency allowlists so a new
edge requires an intentional architecture change. In particular, `sim` must not
depend on `game`, and neither simulation nor gameplay may depend on rendering,
windowing, or the scenario runner.

Gameplay remains deterministic. "Runs in the simulation" does not mean
"belongs in the engine crate." Gameplay also has invariants: awarding a reward
once is a gameplay responsibility, even though it needs strong enforcement.
The distinction is what the invariant governs, rather than whether one exists.

## Ownership rules

- **One definition beside its owner.** Engine definitions remain in `sim`;
  gameplay definitions belong in `game`. File decoders, the harness, and UI
  consume those definitions rather than maintain parallel vocabularies.
  Repository guidance now reflects this boundary and retains its prohibition
  on duplicate definitions.
- **One owner per relationship.** A source-control record owns the association
  between an interactable and a source. Neither endpoint maintains a second
  copy. Scene-local authored references resolve to stable runtime identities
  for each instance; loading a scene twice creates independent connections.
- **Mechanics own their state.** Organize `game` by mechanic, with its state and
  behaviour together. Source control is the first module. Add encounter or
  objective modules only when those mechanics exist. Use explicit phases for
  sequences, and derive facts already available from authoritative state.
- **Game coordinates mechanics.** Keep `Game` responsible for scheduling,
  lifecycle, and adapter entry points. New rules and state transitions belong
  in their mechanic modules, with only the wiring added to Game. Review a new
  mechanic's actual access permissions, not merely which crate contains it;
  moving unrestricted logic from World into Game would repeat the same problem.
- **Passes get restricted access.** A source-control pass can read interaction
  state and request source enablement. It cannot change health or positions.
  Do not give ordinary gameplay passes `&mut Game` or unrestricted `&mut World`.
- **The engine owns enabled state.** Gameplay requests source state changes
  through a validated interface; it does not maintain a duplicate enabled flag.
  The source does not know about shrines, encounters, or victory.
- **Trace is observation.** Gameplay reads typed state or deliberately designed
  interfaces, never the diagnostic trace ring. Avoid a universal event bus,
  string-addressed state store, or scripting framework for this mechanic.

## Execution, lifecycle, and observation

`Game::step` is the complete gameplay entry point for both the app and the
scenario runner. Source control reads the previous completed state and submits
source changes before the next engine step. The engine retains its internal
pass order and structural mutation boundaries. Additional execution phases
require a concrete mechanic and an explicit ordering contract.

For the implemented start button, interaction on tick N permits the first emission
on tick N+1, provided the source is ready and its existing condition is met.
Bodies still enter through the source request queue and normal spawn boundary.
This is the source-control mechanic's contract, not a universal one-tick delay
for gameplay. Each future mechanic must state which completed state it reads,
when its writes become visible, and which structural boundary applies them.
Scenarios must assert those boundaries. Introduce another execution phase only
when a concrete mechanic requires it.

`Game` owns the complete playable scene instance, including gameplay records and
its engine resources. Existing engine ownership and cleanup can remain behind
delegation. Callers must not have to remember two separate cleanup operations.
Validate the complete authored description before installation; failed loading
must leave the current game untouched. Restart and eviction include gameplay
state, engine state, and pending work.

The complete game must be observable, hashable, and replayable. Extend reports,
transition traces, and determinism checks when gameplay state first appears.
Preserve exhaustive integration points so adding a state field demands a
decision about hashing, observation, and lifetime. Engine tests may still drive
`sim` directly; gameplay scenarios must drive the whole game.

## Staged implementation

Each stage is a separate reviewable change. The checkboxes record completion,
not authorization to implement all stages in one turn.

### 1. Extract shared content decoding

- [x] Move shared scene decoding from `scenario` into `content`.
- [x] Make the app and runner depend on `content`; remove the app's dependency
  on the scenario runner crate.
- [x] Update dependency guards and ownership documentation.

Completion: existing files decode the same way, including rejection behaviour;
all existing scenarios pass unchanged. No gameplay or schema changes.

Completed 2026-09-10: `content` now owns shared scene loading and harness
template parsing; their implementations are unchanged. The app and runner use
that library, and `scenario` is binary-only. The new content dependency
allowlist admits only sim and RON. Scene definitions, validation, and
instantiation remain in sim. Clippy, workspace tests, rustdoc, and all 53
existing scenarios passed without changing scene files or golden traces.
The dependency graph and the new guard's rejection paths were also checked.

### 2. Introduce the game entry point

- [x] Add `game` with a `Game` that initially wraps the existing world.
- [x] Route app and scenario stepping through the same entry point; delegate
  observation and extraction without changing their meaning.
- [x] Expose deliberate queries and operations rather than a general mutable
  world escape hatch. Keep the gameplay crate free of the graphics stack.

Completion: existing timing, traces, state observations, and deterministic
behaviour are preserved. Existing scenarios still exercise production logic;
there is no second schedule or test-only gameplay implementation.

Completed 2026-09-10: `Game` privately owns the engine world, and both adapters
construct, step, query, and observe it through explicit methods. The wrapper
preserves engine hashes and report shape; scene operations delegate existing
ownership without adding a second lifecycle store. The gameplay dependency
allowlist permits only core, sim, and glam. Compatibility tests compare default,
empty, and scene construction, then mixed fixed-tick inputs, hashes, traces,
reports, and interpolated positions/facing against the standalone engine. A
compile-fail doctest checks that callers cannot access the private world.
Clippy, workspace tests, rustdoc, and all 53 existing scenarios passed. Engine
code, scenes, golden traces, and protocol/diagnostic string literals are unchanged.
At that checkpoint, complete scene lifecycle ownership was deferred to phase 3.

### 3. Establish complete lifecycle ownership

- [x] Make `Game` the entry point for scene loading, restart, and eviction,
  delegating existing engine cleanup.
- [x] Define the ownership boundary for future gameplay records without
  scaffolding unused mechanics.
- [x] Preserve atomic load failure, independent instances, pending-work cleanup,
  and complete fresh restart.

Completion: lifecycle checks cover the new entry point, including failed
replacement leaving the active game intact and eviction preserving other
instances. No caller needs to coordinate engine and gameplay cleanup manually.

Completed 2026-09-10: Game owns the immutable validated restart snapshot, atomic
start/restart, and exhaustive additive load/evict boundaries. Live bodies,
sources, and queued descendants still use engine ownership. Future gameplay
scene records belong beside the snapshot in `game/scene.rs` and must participate
in these same boundaries; no placeholder instance registry was added. The app
now performs only device/presentation cleanup and run-id advancement after a
successful game operation, with run-id exhaustion checked before mutation.

The new snapshot affects future restart input, so Game's composite hash now
includes its fresh-state fingerprint. State reports add the restart selection;
existing hash values intentionally change. The engine-only fingerprint, scene
files, event traces, and tick behaviour stay unchanged. Compatibility tests now
compare engine fingerprints and assert the additional report fields explicitly.
Public lifecycle tests cover rejection atomicity, preserved and discarded
pending work, cached restart after eviction, independent instances, and restart
choices with identical active worlds. App tests cover cached restart and
run-id exhaustion without cancelling pending capture/input. Clippy, workspace
tests, rustdoc, and all 53 existing scenarios passed.

### 4. Add engine source enablement

- [x] Add a validated source enablement operation in `sim`, independently
  drivable and observable through engine interfaces.
- [x] Keep existing sources enabled by default and retain their current cadence,
  population/proximity conditions, and spawn queue path.
- [x] Specify disabled countdown behaviour and re-enable timing before coding:
  disabling freezes countdown and ring progress; enabling resumes them. Ready
  sources may emit on that evaluation if their condition is met.
- [x] Cover enabled state and any pending state-changing requests in reports,
  hashes, and lifetime handling.

Completion: scenarios assert no emissions while disabled, exact enablement and
cadence timing, composition with existing conditions, and unchanged default
source behaviour. Pin re-enable semantics even though toggling the control is
outside the first playable slice.

Completed 2026-09-11: sim owns source enablement, exposed through explicit World
and Game operations, authored source definitions, and harness enable/disable
commands. Read-only queries, reports, and scenario assertions share the stored
`SourceState` definition. Actual changes emit a typed trace event; repeated
settings leave state and trace unchanged, and stale IDs are refused. The
operation applies immediately and cannot access or cancel accepted spawn work;
there is no pending enablement queue.

Three new scenarios pin disabled inactivity, exact resumed cadence and ring
progress, and composition with population/proximity gates. Engine and public
Game tests cover hashing, observation, idempotence, accepted requests, removal,
independent instances, eviction, and authored restart. CLI tests reject invalid
commands and ensure failed intermediate assertions survive correct final state
and blessing. Clippy, workspace tests, rustdoc, and all 56 scenarios passed;
the original 53 scenarios and golden traces are unchanged. Rustdoc retains its
existing private-link warnings. Engine/game hash values for worlds containing
sources intentionally change because enablement now participates.

The [source enablement contract](source-enablement.md) records timing, lifetime,
and driving examples. The interaction relationship remains phase 5.

### 5. Implement gameplay source control

- [x] Add the interaction-to-source relationship in a small gameplay module.
- [x] Add authored connections, resolving them per scene instance and rejecting
  invalid references before installation.
- [x] Integrate state, trace, hashing, restart, and cleanup in the same change.
- [x] Specify and assert behaviour when either endpoint disappears; a stale
  connection must never affect an unrelated or recycled identity.

Completion (clarified after implementation): scenarios assert inactivity before
activation, activation on N and first permitted emission on N+1, repeated
one-shot input, independent instances, endpoint removal, and eviction. CLI
tests reject invalid references. Public Game lifecycle tests assert restart
and failed-load atomicity. All new gameplay state participates in the runner's
replay checks. The original plan also required restart in the scenario timeline;
that command was not implemented. Its coverage is provided by public API tests
and phase 6's live restart checks, rather than claimed as scenario coverage.

Architecture checkpoint: the relationship must be implementable through narrow
engine interfaces. If it requires broad access to engine internals, adjust the
boundary before building encounters on top of it.

Completed 2026-09-11: `game/source_control.rs` owns resolved connections and
Pending/Started/Orphaned phases. Its named pass runs before the engine schedule
and receives only interaction observation, source enablement authority, its own
records, and a typed gameplay trace sink. Activation on N is consumed on N+1;
Started never reapplies, and missing endpoints retire Pending connections.
The engine schedule is unchanged and no gameplay pass receives World or Game.

`GameScene` composes the existing engine scene definition with authored source
controls. Relationships validate before physical admission, resolve from the
installed instance's IDs, and retire at the same eviction boundary. Sources
must start disabled and have a single controller. Legacy files remain valid;
content decoding and dependency allowlists now include game-owned definitions.
Reports, deterministic hashes, complete restart snapshots, and separate typed
diagnostic streams include gameplay state without duplicating engine enablement.

Three scenarios cover exact activation timing, repeated input, external pause,
independent instances, eviction before consumption, endpoint removal, and
recycled body storage. CLI rejection tests cover malformed content and invalid
assertion/command targets. Public Game lifecycle tests cover failed-load
atomicity, restart after pending and consumed activation, stale source IDs, and
cached relationship hashes. Restart remains verified at the public Game boundary;
the scenario timeline does not yet offer a restart command.

Clippy, workspace tests including the GPU test, rustdoc, and all 59 scenarios
passed. The original scenarios and golden traces are unchanged. Rustdoc retains
existing private-link warnings. See [source control](source-control.md).
The minimal connected fixture is `scenes/source_control.ron`; live playtesting
and tuning remain phase 6.

### 6. Author and playtest the scenario

- [x] Build the control-block and dormant-source scene using the shared authored
  format and the existing interaction colour change.
- [x] Add durable assertions for the intended sequence, then exercise the live
  scene through the picker and harness.
- [x] Tune control placement, spawn placement, cadence, and population limits.

Completion: the scenario runner asserts the behaviour and exits 0; the live
playtest demonstrates interaction and spawning with visible feedback. Report
behavioural verification separately from the user's judgement of combat feel.

Completed 2026-09-11: `scenes/activate_horde.ron` is the playable trial, separate
from the unchanged fast timing fixture. The control begins within reach;
seekers approach from an eight-metre offset on a 2.5-metre ring, every 45 ticks,
with a population limit of eight. A shared-file scenario asserts dormancy,
activation on N and emission on N+1, cadence, the cap, and refill after removal.

Live release testing selected the scene through F2 and exercised E, movement,
Basic attacks, and both picker and harness restart. Screenshots confirmed the
blue-to-green transition and visible enemy approach. Traces pinned the first
emission one tick after activation, 45-tick cadence, and replacement after six
combat defeats. Both restart paths restored a dormant source and empty arena.
Clippy, workspace tests, rustdoc, and all 60 scenarios passed. Existing golden
traces are unchanged; rustdoc retains existing private-link warnings.
See the [repeatable trial and playtest evidence](source-control-playtest.md).
Combat feel remains a human judgement, separate from these verified behaviours.

## Verification for every implementation stage

Declare each requirement's verification path before implementation. Use the
following division, and name the actual test or artifact in the completion
record:

| Requirement | Verification |
|---|---|
| Tick timing, gameplay outcomes, and deterministic replay | Scenario assertions through Game, including intermediate checkpoints |
| Atomic admission, rejection, restart, and complete resource cleanup | Public Game lifecycle tests; scenario assertions also cover lifecycle operations available in its timeline |
| Invalid authored input or unreachable assertions | CLI tests checking nonzero exit, including blessing paths |
| Visible feedback and real binding/picker/harness integration | Live playtest with saved observations and engine screenshots |
| Whether combat feels satisfying | Human playtest judgement, reported separately |

A live check does not replace a durable behavioural assertion. Lifecycle tests
complement scenario coverage; they do not waive the repository's requirement
that every simulation behaviour change have a passing scenario. If a planned
verification path changes, record the deviation and its replacement explicitly.

Run the repository gates before proceeding:

```sh
. "$HOME/.cargo/env"
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --lib --bins --no-deps --document-private-items
cargo run --quiet -p scenario -- scenarios/
```

The test suite requires a real GPU adapter; the scenario runner does not.
Behaviour-preserving stages should keep existing scenario expectations intact.
Any intentional observable change needs a written reason and reviewed assertions;
do not bless trace changes merely to make a refactor pass. New behaviour is
unfinished until a scenario asserts it and exits 0.

## Lessons and next architecture priority

The migration confirmed the value of establishing complete lifecycle ownership
before adding the first gameplay relationship. Validation, restart, and eviction
then had a single integration boundary. Preserve that order for future mechanics,
along with the restricted pass interfaces; crate placement alone is insufficient.

**Named authored references are the next priority before scenes acquire many
relationships.** Current numeric body/source indices validate existence and
capability, but reordering two valid entries can silently connect the wrong
objects. Stable runtime IDs prevent recycling bugs after installation; they
cannot catch an authored relationship that already points at the wrong object.
This is a plausible AI editing failure and deserves stronger enforcement.

The follow-up should introduce scene-local names with distinct body/source
reference types, defined beside the owning authored vocabulary. Resolve names
once during admission to the existing runtime IDs; gameplay passes should
continue using those IDs. Reject duplicate names, unknown references, and
incompatible endpoint capabilities before any installation mutates state.
Specify compatibility for existing numeric files as part of that change.

Completion should include a regression that reorders or inserts unrelated
authored entries while preserving the intended named connection and its gameplay
behaviour. Do not require identical numeric IDs or raw hashes across reordered
descriptions. Also verify duplicate/unknown-name rejection and two instances
using the same local names independently. This follow-up is recorded, not
implemented or added retroactively to the six-phase migration.

## Deferred work

Keep existing combat, movement, and physics in place during this migration.
Do not split every pass into a crate, scaffold future encounter systems, add an
off-the-shelf ECS, or design a general scripting/plugin framework. Revisit other
ownership boundaries after the first gameplay relationship provides evidence.

The success criterion is that the next encounter rule can be added by reading
its mechanic and declared interfaces, without editing engine internals.
