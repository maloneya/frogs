# Crowd collision

The [grid](../crates/sim/src/grid.rs) prunes distant pairs while preserving the
ascending dense-row order and immediate response of the reference all-pairs
sweep. Corrections can create later contacts, so membership updates happen
inside traversal. See the owner for candidate selection and cell-page storage.
The player has a separate linear contact pass after crowd separation.

## Gaits and pushing

Seeking enemies choose Walk, Run or Sprint from stable identity and tick, with
staggered changes. Restart repeats those choices. [`seek`](../crates/sim/src/pass/seek.rs)
owns selection and speeds; `Game::gait` and the `gaits` report expose choices.

Gaits request velocity through the [shared motor](../crates/sim/src/pass/motion.rs).
Contacts exchange that velocity, so faster enemies can push slower ones without
an attack. Knockback survives steering and decays gradually. Arriving enemies
reduce requested speed, but other bodies can still push them. Player controls
remain immediate, with physical knockback added separately.

Horde animation shares bounded poses, reusing the Run clip at gait-dependent
cadences. It has no separate authored walk/sprint clips.

## Measuring and limits

Start **Crowd stress — 10,000 seekers** and wait for compression before following
[performance telemetry](performance.md). Compare crowd time per tick,
candidate/possible-pair counts and frame percentiles. Repeat with movement and
attacks; retain the actual population because defeats reduce workload.

Dense coincident piles, edge cells and repeated cell crossings can still be
costly. There is no candidate cap that drops contacts. Grid scratch is rebuilt
each pass, so it requires no persistent identity repair after removal.

## Fast crossings

Before integration, [contact response](../crates/sim/src/pass/separate.rs) predicts
pairs that would cross their closest approach this tick and exchanges momentum.
Walls shield pairs. This prevents large impulses from reversing the contact
normal between endpoint checks, but response can occur one tick early.

The scan is skipped when no body travels more than an enemy radius. Sustained
large impulses need crowd-scale performance checks. Motion remains on the ground.
