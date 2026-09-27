# Static props and interaction

In Scenes, open `soul_trial.ron` and **Save & play**. Press E beside the blue
block: it turns green once and starts the [encounter](encounters.md).
Restart restores Ready. The [plinth](../assets/world/README.md) has circular
collision independent of its visible mesh.

A scene body can grant interaction independently of fixed physics:

```ron
(pos: (0.0, 2.0), what: (fixed: true, interactable: true))
```

Fixed props have zero inverse mass, no health, and cannot seek. A movable enemy
can also be interactable. Props share body capacity but are excluded from enemy
counts and source population gates.

E activates the nearest Ready object in range after movement and collisions.
Ties prefer the lower stable entity slot; facing is irrelevant. See
[the interaction owner](../crates/sim/src/pass/interact.rs) for reach and selection.

The harness uses the same templates:

```text
spawn (pos: (0.0, 2.0), what: (fixed: true, interactable: true))
source (pos: (4.0, 0.0), every: 120, what: (fixed: true, interactable: true))
tap e
```

`state.sim.interactions` reports activation by stable body ID;
`interaction_reach` reports the shared reach. Position, velocity and inverse mass
remain under `state.sim.bodies`. Scene eviction removes interaction memberships;
a fresh body starts Ready.
