# Why we build this way

[CLAUDE.md](../CLAUDE.md) contains the working rules. These principles explain
how scene authoring, engine work and verification fit together.

## Build through play

Scenes are the working material of the game. Compose a situation in the
[editor](scene-editor.md), play it, adjust placement or tuning, and repeat.
Prefer a new arrangement of existing rules over code written for one encounter.
A row of enemies beside a wall can reveal more about knockback than another
abstract mechanic proposal.

The engine supplies validated rules; the scene combines them. The companion and
agents author complete documents through the [same headless API](scene-authoring.md).
Every authorable concept must work through both. The UI owns drafts and history;
Game receives only playable data, with no editor state or process dependency.
A missing control is a tooling gap. Add a mechanic when a playtest needs a rule
the current vocabulary cannot express, and expose it through both authoring paths.

Keep authored content separate from its running instance. A death or moved enemy
must not silently rewrite the starting layout. A scene can be a disposable
experiment or durable game content; both use the same format and load boundary.

## Rules that compose

An attack submits an impulse. Shared physics carries motion through contacts,
so the struck enemy can push another body outside the hitbox. Putting that
transfer inside the attack would make it available only to attacks.

Sim owns physical mechanisms; game owns relationships such as which interaction
enables a source. Test the mechanism and its meaningful combinations. Consistent
rules allow surprising play without a special case for every pair of mechanics.
This does not require a general framework or a scripting language.

## Make mistakes difficult

A private validated value protects every caller, including files and tooling.
A pass given an impulse sink cannot overwrite positions. A scene owner can
remove relationships and their resources together. Prefer these safeguards over
instructions a caller may forget; see [enforcement](invariants.md).

Types cannot prove timing or gameplay outcomes. Scenarios drive the same Game
with predicted inputs and assertions. Replay establishes repeatability, not
correctness; traces explain transitions, but never drive gameplay. Tests own
minimal setups for individual systems and a small set of complex test-only
compositions for interactions. Playable scenes are freely editable and are not
automated test inputs. A regression discovered while playing becomes a focused
test of the underlying rule, not a contract for that scene layout.

## Learn through small changes

Explain the rule, its visible effect and the tradeoff. For example, fixed ticks
keep attack duration independent of rendering speed; interpolation smooths the
picture without changing combat. A short cause-and-effect explanation is more
useful than a tour of every file.

Deliver a running change and a repeatable way to try it. Separate mechanical
verification from the user's judgement of feel. Neither a passing scenario nor
an editor feature establishes that combat is fun.
