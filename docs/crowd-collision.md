# Crowd collision

The [grid](../crates/sim/src/grid.rs) prunes distant pairs while preserving the
ascending dense-row order and immediate response of the reference all-pairs
sweep. Corrections can create later contacts, so membership updates happen
inside traversal. See the owner for candidate selection and cell-page storage.
The player has a separate linear contact pass after crowd separation.

## Movement and pushing

Game-owned [behavior actions](behaviors.md) choose target velocity and acceleration.
The [shared motor](../crates/sim/src/pass/motion.rs) applies those requests once
per actor per tick. Contacts exchange physical momentum, so faster actors can
push slower ones without an attack. Knockback survives steering and decays
gradually. Arrival braking reduces requested speed; other bodies can still push
an arriving actor. Player controls remain immediate, with knockback added separately.

Horde animation derives walk/run/sprint cadence from actual displacement and
shares bounded poses. It has no separate authored walk/sprint clips, and its
visual choice never drives gameplay.

## Measuring and limits

Start **Crowd stress — 10,000 pursuing actors** and wait for compression before following
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
