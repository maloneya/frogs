# Props and interaction

Place a **Switch** in the [scene editor](scene-editor.md), then **Save & play**.
Press E beside the blue plinth: it turns green once. Bind it as an
[encounter](encounters.md) activation body to start an objective. Restart restores
Ready. The [plinth](../assets/world/README.md) has circular collision independent
of its visible mesh.

A scene body can grant interaction independently of fixed physics:

```ron
(pos: (0.0, 2.0), what: (physical: (fixed: true, interactable: true), role: Prop))
```

Fixed bodies have zero inverse mass and cannot use movement actions. Health,
interaction and gameplay role are independent capabilities; movable actors can
also be interactable. Props share body capacity but are excluded from hostile
population counts and source population gates.

E activates the nearest Ready object in range after movement and collisions.
Static walls and every jointed body, such as [door panels](physical-doors.md)
and [trunks](felled-trees.md), block interaction through their current geometry. Releasing a door's latch does not clear that
obstruction; moving the panel aside does. A panel does not obstruct interaction
with its own endpoint. Ties prefer the lower stable entity slot; facing is irrelevant. See
[the interaction owner](../crates/sim/src/pass/interact.rs) for reach and selection.

The harness uses the same templates:

```text
spawn (pos: (0.0, 2.0), what: (physical: (fixed: true, interactable: true), role: Prop))
source (pos: (4.0, 0.0), every: 120, what: (physical: (fixed: true, interactable: true), role: Prop))
tap e
```

`state.sim.interactions` reports activation by stable body ID;
`interaction_reach` reports the shared reach. Position, velocity and inverse mass
remain under `state.sim.bodies`. Scene eviction removes interaction memberships;
a fresh body starts Ready.
