# Working on arpg

Repository guidance for coding agents. This is a from-scratch action-RPG engine
and a learning project for a novice game developer, focused on combat against
large crowds. Understanding the systems matters alongside getting them running.

## Design priorities

- **Simulation first.** Build consistent rules that compose. Players should be
  able to discover interactions we did not script individually. An attack asks
  the shared physics system for an impulse; contacts carry that motion onward.
  Put shared mechanisms below their producers and test their combinations.
- **Teach through the work.** Explain the relevant concept in plain language,
  connect the design to an observable gameplay effect, and name the tradeoff.
  Use a small example when helpful. Distinguish verified behaviour from a feel
  judgement for the user to make. Avoid unexplained jargon and long lectures.
- **Small running changes.** Build one system at a time. Do not scaffold future
  mechanics or generalize without a concrete need.
- **First principles.** Hand-write the engine layers being studied: simulation
  loop, entity storage, spatial partitioning, renderer. Crates for math and
  plumbing are fine; an off-the-shelf engine or ECS is not.

[Design rationale](docs/agent-principles.md) explains how these priorities fit.

## Rules while editing

1. **Enforce at the owner.** Assume a caller or future agent will forget a rule.
   Prefer private fields, narrow capabilities and validated constructors, then
   compile-time checks, runtime validation and tests. Prose is the last defence.
   Enforce at the shared entry point, including file and harness input. Explain
   why a weaker safeguard is necessary. See [enforcement](docs/invariants.md).
2. **Keep one definition.** The owning system defines types, validation and
   tuning. Content, UI, reports and harness consume that vocabulary. Derive
   facts from authoritative state; do not maintain a second copy by convention.
3. **Keep execution deterministic.** Simulation and gameplay depend only on
   state and ordered inputs: no wall clock, unseeded randomness or hash-ordered
   iteration. Presentation may interpolate, smooth and animate but cannot write
   back into simulation. Replay is scoped to one binary on one machine.
4. **Declare access and order.** Behaviour belongs in named passes with the
   slices or restricted interfaces they need, never unrestricted mutable World
   or Game access. Step methods wire passes; they do not contain mechanic logic.
   State when a pass reads, when its effects land, and who owns structural edits.
5. **Make systems independently testable.** Provide typed driving operations,
   read-only observations and transition traces through engine/game interfaces.
   Gameplay reads authoritative state, never the diagnostic trace ring.
6. **Close the lifecycle.** New state needs explicit spawn, removal, scene
   eviction, restart, hash and report handling. Use stable identities across
   dense-row moves. Validate complete loads before mutation; rejected input must
   preserve live state and accepted pending work.
7. **Verify outcomes.** A simulation or gameplay behaviour change requires a
   passing scenario through Game, with predictions made before observing output.
   Check intermediate ticks and interactions with existing systems. Test refused
   input and stale identities where relevant. Never weaken a gate or bless a
   trace merely to make a change pass.
8. **Diagnose from evidence.** Search [traps](docs/traps.md) before investigating
   a symptom. After a misleading failure costs over ten minutes, record its
   symptom, cause and check; replace recurring advice with executable safeguards.

## Ownership

The engine protects shared simulation mechanisms; gameplay owns the rules and
relationships of a mechanic. Both are deterministic and need invariants. This
engine serves one game; hypothetical reuse is not a reason to add abstraction.
Existing combat tuning still lives beside engine validation in sim; read the
owner before moving it. New gameplay relationships belong in game.

| Crate | Responsibility |
|---|---|
| core | Small shared vocabulary, independent of graphics and gameplay relationships |
| sim | Physical world, identity, capabilities, validated operations and pass schedule |
| game | Playable state, mechanic relationships, orchestration and complete scene lifetime |
| content | Decode authored definitions supplied by sim and game |
| assets | Validate bounded GLB input into CPU assets, independent of gameplay and GPU resources |
| gfx | Rendering, camera and GPU resources; receives anonymous geometry |
| app | Devices, UI, presentation and harness; maps actions to world-space intent |
| scenario | Headless driving and assertions through the same Game used by app |

Game privately owns World. App and scenario use Game for construction, stepping
and lifecycle; engine tests may drive World directly. Dependency allowlists in
crate build scripts enforce permitted edges. One world unit is one metre;
see [world units](docs/world-units.md).

Native macOS / Apple M4 / Metal is the target. Cross-platform and wasm support
are out of scope. Preserve these presentation contracts when editing: colours
are linear with sRGB output, camera smoothing uses elapsed-time half-lives,
horde animation shares bounded pose buckets, redraw is continuous, and depth
is discarded after the pass because nothing reads it. Read the relevant owner
and its tests before changing these choices.

## Checks and commands

Rust may need `. "$HOME/.cargo/env"` before commands. Run these checks before
finishing a change; report any failure or check you could not run:

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --lib --bins --no-deps --document-private-items
cargo run --quiet -p scenario -- scenarios/
```

Workspace tests include GPU tests and require a real adapter. Scenarios need no
GPU or window. Use `#[expect(lint, reason = "…")]` for a justified exception,
never `#[allow]`. Do not relax lint or dependency policies to get a build through.

Claude Code hooks in [.claude/settings.json](.claude/settings.json) run some of
these checks automatically. Other agents and shells must run them explicitly;
the presence of that file does not prove a check ran. Rustdoc needs its own
command; passing clippy and tests does not validate documentation links.

`cargo run` launches the app. Use `cargo run --release` for performance work.
Drive live checks through `ARPG_HARNESS`, not OS key injection or desktop capture:

```sh
ARPG_HARNESS=/tmp/arpg.sock cargo run --release
echo 'state' | nc -U /tmp/arpg.sock
```

## Task guides

Read only the guides relevant to the work:

- [Add or change a sim pass](.claude/skills/add-sim-pass/SKILL.md)
- [Write and run scenarios](.claude/skills/scenario/SKILL.md)
- [Live playtesting, capture and performance](.claude/skills/playtest/SKILL.md)
- [Author a scene](.claude/skills/create-scene/SKILL.md) and [scene lifecycle](docs/scene-playtests.md)
- [Author Blender assets](.claude/skills/author-blender-asset/SKILL.md) and [asset contract](docs/character-assets-authoring.md)
- [Interaction](docs/interaction.md), [source enablement](docs/source-enablement.md), [source control](docs/source-control.md)
- [Collision debug view](docs/collision-debug.md) and [attack effects](docs/attack-effects.md)

Keep documentation beside its subject. Explain non-obvious constraints near the
code that enforces them; task guides explain how to use them. Link rather than
copying schemas, tuning catalogs or test inventories. Remove obsolete guidance
when behaviour changes; keep completed implementation history in version control.
