# Finding and strengthening safeguards

An invariant is a property that must remain true after every allowed operation.
Put its enforcement beside the data owner, at the entry point every caller uses.
The editing rules are in [CLAUDE.md](../CLAUDE.md); this is a guide to finding
and evaluating enforcement, not a second inventory of the code.

## Choose the strongest practical mechanism

| Layer | Mechanism | What to check |
|---|---|---|
| 0 | Private fields, ownership, narrow capabilities | Can any public entry point construct invalid state or bypass the owner? |
| 1 | Types, const assertions, lints, dependency build guards | Does the plausible wrong edit fail compilation? |
| 2 | Runtime admission and GPU validation | Are invalid inputs refused before partial mutation? |
| 3 | Unit, integration, scenario and pixel tests | Does a meaningful wrong result fail, including at intermediate ticks? |
| 4 | Prose and review | What cannot yet be enforced, and why? |

Use more than one layer when they protect different properties. A constructor
can reject invalid impulse values; a scenario must still prove when the impulse
moves a body. A debug assertion does not protect a release build. A test or
lint helps only when its command actually runs.

## Where to look

| Concern | Owning implementation and checks |
|---|---|
| Crate boundaries and denied lints | [workspace manifest](../Cargo.toml), crate build scripts, [shared dependency guard](../build_support/dependencies.rs) |
| Deterministic code's prohibited APIs | [shared Clippy configuration](../build_support/deterministic-clippy.toml), linked from sim/game; diagnostics explain the owning layer |
| Stable identity and capability lifetime | [slots](../crates/sim/src/slots.rs), [members](../crates/sim/src/members.rs), body creation/removal in [World](../crates/sim/src/lib.rs) |
| Fixed time and read-only presentation | [time](../crates/sim/src/time.rs), World presentation methods and adjacent tests |
| Physical queries and restricted effects | [contact](../crates/sim/src/contact.rs), [motion](../crates/sim/src/pass/motion.rs), [attack](../crates/sim/src/pass/attack.rs) |
| Complete game lifecycle | [Game](../crates/game/src/lib.rs), [game scenes](../crates/game/src/scene.rs), public lifecycle tests |
| Shared authoring admission and persistence | [SceneDocument::validate](../crates/content/src/document.rs), [authoring API](../crates/authoring/src/lib.rs), [CLI round trips and refusals](../crates/authoring/tests/cli.rs) |
| Replay, checkpoints and rejected assertions | [scenario execution](../crates/scenario/src/run.rs), [scenario schema](../crates/scenario/src/spec.rs), CLI tests and [scenarios](../scenarios) |
| Asset admission and rendering contracts | [assets](../crates/assets/src/lib.rs), [gfx](../crates/gfx/src/lib.rs), importer and GPU tests |
| Rust API documentation links | Required rustdoc command with broken intra-doc links denied |

Exhaustive destructuring forces an edit when fields are added; it does not prove
the author hashed, reported or cleaned up each field correctly. Inspect the
implementation and its tests. Rustdoc checks Rust API links; prose claims and ordinary Markdown links still
need review.

Clippy runs through the existing verification runner and agent hooks. Workspace
rules reject unnecessary mutable parameters, redundant clones, redundant `..`
in otherwise exhaustive destructuring, and outer `#[allow]` attributes. Lint
exceptions need a reason; unfulfilled expectations fail so stale exceptions do
not silently accumulate. Sim/game also reject wildcard enum match arms: a new
state needs an explicit decision. Intentionally partial observations can use a
narrow, justified `#[expect]`.

Sim/game share one Clippy configuration through their `clippy.toml` symlinks.
It prohibits named clock, environment, unordered collection, I/O and process/thread
entry points, including renamed imports. Their build-time dependency guards have
an explicit exception for reading manifests. Other crates retain their adapter
capabilities. These checks run under Clippy, not ordinary `cargo build`; the API
list is not a complete side-effect sandbox, and no lint proves correct lifecycle
handling or detects a second gameplay schema. Keep those checks at the owner and
in outcome tests.

When adding state, follow it through creation, mutation, removal, scene eviction,
restart, observation and hashing. Exercise rejection and identity reuse where
relevant. Keep the exact contract and the reason for any limitation beside the
owner; avoid adding another prose catalog that must track every implementation.
