# Static props and interaction

Open F2, select `activation_block.ron`, and press Enter. Walk toward the blue
block and press E: it turns green once. Restart current restores Ready.
The block uses the existing 0.5 m cube and circular collision footprint; larger
boxes and door-shaped collision are later work.

The pieces are deliberately separate:

- Body storage supplies identity and position. `fixed: true` grants zero inverse
  mass and omits health. It cannot chase. Static placements must be inside the
  arena so containment never moves them after spawn.
- Health is sparse membership keyed by identity. Attack damage cannot reach a
  prop's nonexistent health, and enemy counts and source gates exclude props.
- `interactable: true` grants `InteractionState::Ready` independently of physics.
  This can also be attached to an ordinary movable enemy.
- E becomes an Interact action, then one tick's interaction intent. The named
  interaction pass checks final positions after movement and collisions. It
  chooses one nearest Ready object within 1.5 m centre-to-centre, inclusively;
  equal distances prefer the lower stable entity slot. Facing is not required.
- Activation changes state and emits `activated id=...`. Extraction derives
  green from that state. Future chest/door/beacon systems can consume the state;
  trace output remains observation. There is no event-dispatch framework.

Templates are defined once in sim. A scene placement is:

```ron
(pos: (0.0, 2.0), what: (fixed: true, interactable: true))
```

The same template can be used in scenario spawns, sources, or harness commands:

```text
spawn 0 2 template (fixed: true, interactable: true)
source 4 0 every 120 template (fixed: true, interactable: true)
tap e
```

`template` consumes the rest of the command as RON; put legacy source flags
before it. `state.sim.interactions` is keyed by stable body id and reports
`activated`; `interaction_reach` reports the shared reach. Body position,
velocity and inverse mass remain under `state.sim.bodies`.

Enemy debug resets preserve props and their activation. Scene eviction removes
all owned bodies and revokes their interaction memberships; a fresh scene or a
recycled slot starts Ready. Props consume the same instance capacity as enemies.
Invalid static placement or contradictory fixed/seek grants are refused at the
body storage boundary; authored scenes validate all placements before mutating
the world. Enemy resets use ordinary body removal, so capability cleanup has
one implementation.

Scenarios cover static collision and immunity, reach and activation timing,
nearest-target selection and ties, source gates, queued grants and eviction.
The colour change is presentation and is checked through the live harness.
