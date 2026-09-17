# One-shot source control

The relationship lives in `crates/game/src/source_control.rs`. Sim owns
interaction membership, source enablement, and physical scene resources.
Game owns which interactable starts which source and whether that request has
already been consumed.

`Game::step` first runs the named source-control pass, then the unchanged engine
schedule. The pass reads the previous completed interaction state through
`InteractionView` and can change only source enablement through `SourceEnablement`.
Neither capability exposes World, body storage, health, or the spawn queue.
The pass never reads diagnostic events.

Activation on tick N leaves the connection Pending through that tick. Before
engine tick N+1, it changes to Started and enables the source. A ready source
may then emit, subject to its normal condition. Started is terminal: later
interaction or an external disable command cannot make the connection apply
again. Engine enablement is never duplicated in the gameplay record.

A missing interaction or source endpoint changes a Pending connection to
Orphaned on the next gameplay pass. This also applies if the player activated
the body but it disappeared before the following tick. Stable generational
body IDs and monotonic source IDs prevent replacement objects from inheriting
the connection. Removing an endpoint after Started leaves the historical phase
Started; removing the body does not stop its source. Scene eviction removes
all its records, sources, descendants, and owned queued work immediately.

## Authoring

`GameScene` wraps sim's existing `Scene` rather than copying its fields:

```ron
(
    engine: (
        name: "Source control",
        bodies: [(pos: (1.0, 0.0), what: (fixed: true, interactable: true))],
        sources: [(pos: (20.0, 0.0), radius: 2.0, every: 4, enabled: false)],
    ),
    source_controls: [(body: 0, source: 0)],
)
```

Each `SourceControlSpec` names an explicit body and a source in the same
authored scene. Grid-generated bodies and descendants are not addressable.
The body must be interactable; the source must exist and start disabled.
A source may have only one controller. One body can start several distinct
sources. Invalid references, duplicate source ownership, and invalid physical
content are rejected before installation changes the active game.

References are numeric indices: reordering valid authored entries can silently
change a connection. Review those references when editing lists; admission can
check capability and existence, but cannot infer the author's intended target.

Loading the same description twice resolves independent runtime connections.
After engine admission succeeds, validated indices resolve against that
instance's bodies and sources, without consulting the trace. Restart restores
the complete cached description, Pending phases, and authored disabled sources.
Additive loads, endpoint removal, and eviction cannot edit that snapshot.

The content loader accepts both this wrapped form and existing engine-only
files. Both schemas reject unknown fields. Inline scenarios use `Gameplay(...)`
for the complete form and retain `Inline(...)` for legacy physical content.
Owning types supply their authored definitions to the shared decoder.

## Observing and verifying

`Game::source_control_state` returns the stored phase plus the authoritative
engine `SourceState`. Reports list resolved endpoints and phase under
`source_controls`; source state is reported once under `source_states`.
Live record fields and the complete restart effect participate in `Game::hash`.
`Game::engine_hash` still excludes gameplay relationships.

A `ControlEvent` records Started or Orphaned transitions. Engine events and
gameplay events use the same bounded recorder with different payload types.
The harness and golden trace show engine history followed by a labelled
gameplay stream. Each stream preserves its own ordering; equal tick stamps
across streams do not imply an ordering of external commands. Truncation in
either stream refuses golden comparison, and clearing diagnostics changes no
simulation state.

Scenarios assert activation-to-emission timing, one-shot consumption, independent
instances, eviction before consumption, missing endpoints, and recycled bodies.
CLI tests reject malformed connections and invalid observation targets, even
when blessing. Public Game tests also exercise restart after pending and
consumed activation, invalid-load atomicity, cached relationship hashes, and
stale source identities.

The fast fixtures are inline in the regression scenarios. The playable trial is
`scenes/activate_horde.ron`: select it through F2 and press E beside the blue
block. See the [playtest instructions and evidence](source-control-playtest.md)
for the tuned setup, assertions, screenshots, and restart verification.
