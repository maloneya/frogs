---
name: add-sim-pass
description: Add or change a simulation behaviour in the arpg sim — movement, steering, separation, collision, attack state machines, timed hitboxes, knockback, hitstop, spawning. Use whenever a change would otherwise mean editing the body of World::step or adding a stage to the frame loop. Covers the pass contract, where constants and their asserts live, ordering rules, what must be traced, and the scenario required before the change counts as done.
---

# Adding a simulation pass

A behaviour is a **named pass** in the sim schedule. It is not a block of code
appended to `World::step`, and it is not a stage added to the frame loop in
`app`.

The reason is not tidiness. A pass written into `step()` carries its ordering in
control flow, where nothing can read it, assert on it, or stop the next edit from
getting it wrong. A pass that takes `&mut World` can touch anything, so "this
pass only reads positions" is a claim rather than a fact. Both problems are
avoidable at layer 0–1 for free.

## The contract

**Take the data, not the world.** A pass declares what it touches in its
signature and gets exactly that; the borrow checker then enforces the
declaration at layer 0, with no test needed. The passes in
`crates/sim/src/pass/` are the worked examples — copy their shape:

```rust
pub(crate) fn contain(player: &mut Vec2, horde: &mut [Vec2], mut trace: TraceSink<'_>) { … }
```

Not `fn contain(world: &mut World)`.

One honest limit: the player is still a single struct rather than a row in the
horde's storage, so passes take its individual fields where the horde gets a
real slice. The horde half of each signature is checked by the compiler, the
player half by reading it.

**Take `Dt`, never `f32`.** `Dt` carries no number at all and can only have come
from `Accumulator::pending`, so a variable timestep is not something a caller
can express.

**Register, do not inline.** `World::step` is a list of calls into `pass/` and
contains no logic. If your change adds logic to `step`, it is in the wrong
place. `pass/mod.rs` documents the order and why each adjacency is what it is;
a new pass adds its reason there.

**Own your constants, with their asserts.** A constant describing a *behaviour*
— speed, turn rate, mass ratio — lives in the pass module beside the code that
reads it, each with a `const _: () = assert!(…)` covering its valid range. A
constant describing an *entity* — radius, scale, spacing — stays with the
storage. The plausible wrong edit should fail to compile.

**Be a pure function of its inputs.** No wall clock, no `Instant`, no unseeded
randomness, no iteration over a hash-ordered container. Where a pass must break
a tie, derive it deterministically; `escape_direction`'s golden angle is the
pattern to copy.

## Before writing it

Read the passes either side of where yours will sit. Ordering in a fixed-step
sim is semantic: integrating before resolving separation and resolving after
give different games, and both compile.

Say in one or two sentences what the pass does and why it belongs at that point
in the order. That sentence is the thing worth reviewing; the code usually is
not.

## What the pass must emit

Any state transition that a scenario will need to assert on is a trace event,
tick-stamped, emitted by the pass:

```
412 attack.startup
417 hitbox.active
418 hit e=93 impulse=(4.1,-4.1) hp=7
419 hitstop 4
```

This matters most for exactly the behaviours this skill covers. A mistimed
hitbox, a knockback applied on the wrong frame and a hitstop that never releases
all produce no crash, no compiler error and no failing unit test. A `state`
snapshot read afterward cannot see a three-frame error inside a twelve-frame
window. The trace can.

`state` is derived now, so a *world field* becomes observable by existing. An
**event** is for something that happened, which a field cannot represent.

Add a variant to `sim::trace::Event` with a `Display` arm, take a
`TraceSink<'_>` in your signature, and `emit`. The sink already knows the tick,
so an event cannot be stamped with the wrong one.

**Summarise per tick; do not emit per body.** `separate` reports
`contacts count=37`, not thirty-seven events. A pass emitting per body fills the
ring buffer in seconds and pushes out the rare events the trace exists for.

**Trace anything that changes state outside a tick**, as `set_enemy_count` does.
Between-tick writes are the hardest to account for afterwards, and uncapped most
frames run zero ticks — so they are also the most likely to be drawn before
anything has run.

## Done means

1. `cargo clippy --workspace --all-targets -- -D warnings` clean. (The
   `PostToolUse` hook runs this for you after every edit *and* every Bash call.)
2. `cargo test --workspace` clean.
3. **A scenario asserts the new behaviour and the runner exits 0.** See the
   `scenario` skill.
4. If the pass has a cost that scales with the horde, it carries a perf
   assertion in that scenario.

Point 3 is the one that is easy to skip and is the whole point — rule 4 in
`CLAUDE.md` says why. The runner exists; there is no version of this step that
consists of reading `state` and being satisfied.

Two things it gives you free, so do not hand-roll them: every scenario is
replayed and hash-compared per tick, and a scenario naming a golden trace turns
a timing change into a reviewable diff. Add `trace: "name.trace"` to `expect`,
run once with `--bless`, then **read the file before committing it**.

## Feel is not in scope here

A scenario proves the knockback impulse was 4.1 at tick 418. It cannot say
whether 4.1 *feels* right. That is the owner's call, at the keyboard, under
vsync. State the effect you expect, change the constant, measure it, and put the
number in the commit message.
