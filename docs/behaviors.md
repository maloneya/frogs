# Actor behaviors

An actor carries one [definition](../crates/game/src/behavior/definition.rs):
physical capabilities, gameplay role, available actions, and a composition that
selects those actions. The same definition works on an explicit placement, a
formation, a spawn source, or a queued birth. Equal definitions share immutable
storage; each actor has independent progress, targets and cooldowns.

A behavior can select one action, run children in sequence, or choose the first
admissible child by priority. A monster can prioritize swipe, leap, then pursuit.
A civilian can sequence movement to a point, waiting, and interaction. Movement
can yield to a higher-priority action; committed operations retain control until
completion or cancellation. The definition owner validates references, physical
requirements, bounded composition and tuning before admission. Refusal preserves
accepted state and queued work.

Health, mobility and character presentation are independent physical capabilities.
Gameplay role controls targeting and the hostile population used by source gates.
A civilian does not become an enemy because it moves or has health. A character
without health still renders, with no health bar. Player health uses the same
physical damage authority as other actors; reaching zero retains the current
playtest policy of allowing player input.

The [Game schedule](../crates/game/src/game.rs) applies prior interactions, admits
births, and binds their complete definitions before decisions. Decisions observe
the current controlled player movement and other actors' preceding positions.
Player input and autonomous actions use the same [execution lifecycle](../crates/sim/src/action_execution.rs)
for phase timing, control ownership, completion and cancellation.
Autonomous actions submit one bounded motor request per actor. Shared physics resolves
movement, impulses, contacts and walls. Player abilities resolve before autonomous
effects, so an actor defeated by the player cannot land a late hit. The shared
effect boundary refuses stale or already-defeated targets; final removal produces
one defeat. Interactions use the same reach and wall checks for every initiator.

[Coordination and execution](../crates/game/src/behavior/runtime.rs) are separate
from behavior selection. Combat reservations include recovery and stagger new
commitments. Moving, waiting and interacting do not consume combat reservations.
Swipe commits a direction; leap commits a landing point. Leap uses ordinary ground
physics plus a presentation arc, so walls and contacts still apply.

The native inspector and [headless API](scene-authoring.md) edit the same typed
actions and tree. Reordering actions remaps tree references. Use a complete Actor
edit when a change needs several fields to become valid together. Reports expose
actor execution and future spawn definitions; presentation only reads observations.
Runtime movement speeds belong to actions, while player controls and ability
impulses remain in the shared tuning panel.

[Game tests](../crates/game/tests/behavior_composition.rs) own small independent
setups. [Birth tests](../crates/game/tests/actor_births.rs) cover all admission paths
and simultaneous lethal effects. Playable scenes remain human playtests rather
than automated fixtures.
