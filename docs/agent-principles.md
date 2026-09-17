# Why we build this way

[CLAUDE.md](../CLAUDE.md) contains the working rules. This page explains the
three design priorities behind them.

## A world governed by rules

Simulation first means that outcomes follow from the state of the world and
its rules. Shared mechanisms let different mechanics affect each other without
requiring a special case for every pair. That gives players room to discover
uses the developer did not anticipate.

For example, a swing submits an impulse. Physics carries velocity, transfers
momentum through contacts and settles motion. A struck enemy can push another
body outside the hitbox because both participate in the same rules. Putting
that transfer inside the attack would make it available only to attacks.

Separate the physical mechanism from the gameplay relationship using it. Sim
owns source enablement; game owns which interaction starts a source. This lets
us test enablement by itself, then test the composed mechanic and its timing.
It also keeps rendering and animation from becoming hidden gameplay inputs.

Composition does not require a general framework. Start with the concrete
interaction, identify the shared rule, and give each participant only the
access it needs. Verify both isolated behaviour and a meaningful combination.
Consistent outcomes can be surprising to the player without being accidental
consequences of invalid state.

## Make mistakes difficult to express

An agent can forget a convention, duplicate a schema, omit cleanup or accept a
plausible-looking output. More emphatic instructions do not prevent those errors.
The design should reject them at the narrowest shared boundary.

A private validated value protects every caller. A pass given an impulse sink
cannot overwrite positions. A scene operation owning all resources can clean
up the relationship and its endpoints together. These safeguards reduce how
much context an agent needs to make a local change correctly.

Types cannot prove every behavioural property. Timing, deterministic ordering
and interactions need executable examples: predict an outcome, assert the
relevant ticks, and verify that a plausible wrong implementation would fail.
Replay proves repeatability, not that the repeated result is the intended one.
A golden trace needs review for the same reason.

Read-only reports explain what exists; traces explain what happened between
observations. Neither substitutes for assertions. Keep diagnostic data separate
from gameplay inputs so observation cannot alter the system under test.

No repository is made unbreakable by documentation. The useful question is:
what catches this specific mistake, and can a caller bypass that safeguard?
[The enforcement guide](invariants.md) points to the mechanisms to inspect.

## Learn through small, working systems

The user is learning game development. A change should leave them with an
understanding of the system as well as working code. Explain the concept when
it becomes relevant, show how it affects play, and identify the tradeoff in the
chosen design. Define unfamiliar terms before relying on them.

For example: a fixed timestep advances the world in equal time intervals. That
makes attack duration independent of rendering speed; interpolation smooths the
picture between those updates without changing the result. This connects the
implementation to something the player can see and the scenario can assert.

Keep explanations proportional to the change. A short cause-and-effect example
usually teaches more than a tour of every file. Deliver one running system,
show how to exercise it, and report what was tested. Whether combat feels good
remains a human playtest judgement, distinct from mechanical correctness.
