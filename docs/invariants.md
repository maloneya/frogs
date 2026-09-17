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
| Stable identity and capability lifetime | [slots](../crates/sim/src/slots.rs), [members](../crates/sim/src/members.rs), body creation/removal in [World](../crates/sim/src/lib.rs) |
| Fixed time and read-only presentation | [time](../crates/sim/src/time.rs), World presentation methods and adjacent tests |
| Physical queries and restricted effects | [contact](../crates/sim/src/contact.rs), [motion](../crates/sim/src/pass/motion.rs), [attack](../crates/sim/src/pass/attack.rs) |
| Complete game lifecycle | [Game](../crates/game/src/lib.rs), [game scenes](../crates/game/src/scene.rs), public lifecycle tests |
| Replay, checkpoints and rejected assertions | [scenario execution](../crates/scenario/src/run.rs), [scenario schema](../crates/scenario/src/spec.rs), CLI tests and [scenarios](../scenarios) |
| Asset admission and rendering contracts | [assets](../crates/assets/src/lib.rs), [gfx](../crates/gfx/src/lib.rs), importer and GPU tests |
| Documentation citations | [identifier check](../crates/scenario/tests/docs_cite_real_code.rs) and the required rustdoc command |

Exhaustive destructuring forces an edit when fields are added; it does not prove
the author hashed, reported or cleaned up each field correctly. Inspect the
implementation and its tests. Similarly, the documentation identifier check
only searches for cited names; it cannot prove a semantic claim or resolve every
Markdown link.

When adding state, follow it through creation, mutation, removal, scene eviction,
restart, observation and hashing. Exercise rejection and identity reuse where
relevant. Keep the exact contract and the reason for any limitation beside the
owner; avoid adding another prose catalog that must track every implementation.
