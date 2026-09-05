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

**Take slices, not the world.** A pass declares what it touches in its
signature and gets exactly that. The borrow checker then enforces the
declaration — layer 0, no test needed:

```rust
pub(crate) fn separate(
    positions: &mut [Vec2],
    radii: &[f32],
    inv_mass: &[f32],
    dt: Dt,
) { … }
```

Not `fn separate(world: &mut World, dt: f32)`.

**Take `Dt`, never `f32`.** `Dt` has a private constructor, minted only by the
fixed-timestep accumulator. Accepting a bare float is how a variable timestep
gets smuggled back into the sim.

**Be a pure function of its inputs.** No wall clock, no `Instant`, no unseeded
randomness, no iteration over a hash-ordered container. Where a pass needs to
break a tie — coincident bodies, spawn placement — derive it deterministically;
`escape_direction`'s golden angle is the pattern to copy. If a pass genuinely
needs randomness, it takes a seeded generator from the world, and the seed is
recorded with the input stream.

**Own your constants, with their asserts.** Tuning constants live in the pass's
module beside the code that reads them, each with a `const _: () = assert!(…)`
covering its valid range. The plausible wrong edit — a negative speed, a zero
rate, a spacing narrower than the bodies — should fail to compile rather than
produce silently wrong behaviour.

**Register, do not inline.** Add the pass to the ordered schedule. If your pass
must run before or after another, that dependency belongs written down at the
registration site, not implied by where you happened to paste it.

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

Do not add a field to the hand-maintained `state` format string to make
something observable. Emit an event.

## Done means

1. `cargo clippy --workspace --all-targets -- -D warnings` clean. (The
   `PostToolUse` hook runs this for you after every edit *and* every Bash call.)
2. `cargo test --workspace` clean.
3. **A scenario asserts the new behaviour and the runner exits 0.** See the
   `scenario` skill.
4. If the pass has a cost that scales with the horde, it carries a perf
   assertion in that scenario.

Point 3 is the one that is easy to skip and is the whole point. Driving the
game through the harness, reading `state`, and satisfying yourself the numbers
look right is not verification — it is the agent grading its own homework, and
it reads from the inside exactly like success. Predicting a value and asserting
it in a scenario is the same act made durable.

If the scenario runner does not exist yet (roadmap chunk 3), say so explicitly
rather than quietly substituting a manual harness check, and add the unit test
that most nearly covers the behaviour over time.

## Feel is not in scope here

A scenario proves the knockback impulse was 4.1 at tick 418. It cannot say
whether 4.1 *feels* right. That is the owner's call, at the keyboard, under
vsync. State the effect you expect, change the constant, measure it, and put the
number in the commit message.
