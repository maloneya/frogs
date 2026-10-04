# Crowd collision

The [grid](../crates/sim/src/grid.rs) sorts bodies into one-metre cells once per
collision pass and visits each nearby pair exactly once, in an order fixed by
positions and row order. Walls keep their own [cell index](../crates/sim/src/wall.rs),
rebuilt only when the wall set changes. The [shared solver](../crates/sim/src/solver.rs)
resolves player, crowd, wall and panel contacts together, so pressure can travel
through connected bodies to a [door latch](physical-doors.md).

## Soft step

The solver follows Box2D v3's soft step, described in
[Erin Catto's Solver2D](https://box2d.org/posts/2024/02/solver2d/). Each tick:

1. **Collide** once per collision pass. Every pair whose gap could close during
   the pass gets a *speculative* contact, so bodies stop at the surface instead
   of overlapping first and being pushed back out.
2. **Substep** four times. Each substep applies a quarter of every motor's
   change, reapplies the previous substep's impulses (warm start), solves once
   with soft overlap correction, moves bodies, and relaxes once without
   correction so the push leaves no velocity behind.
3. **Store** each contact's force for the next tick's warm start.

Work per contact is fixed: nothing iterates to a tolerance. Steady loads still
come out exact because warm starting carries last tick's answer forward. A
blocked chain of motors delivers its combined load to a latch within 0.1%.

Observable consequences:

- **Contacts are inelastic.** Bodies that meet share their closing speed; nothing
  bounces (Box2D's default of no restitution).
- **One sweep per phase propagates a blow approximately.** When a moving body
  strikes a touching row, all the momentum arrives, but the struck bodies can end
  slightly faster than the striker and drift ahead. Momentum is conserved exactly.
- **Steady light loads are rigid.** Overlap within a half-millimetre slop is held
  rigidly, so a player leaning on a wall or two enemies shoving each other rest
  at touching distance. Heavy loads compress softly, like a spring of stiffness
  m·ω² at the contact frequency, doubled against walls and fixed bodies.

## Contact response

Three [tuning](tuning.md) fields set the global response. They apply to every
contact; gameplay roles do not override them.

| Field | Effect |
|---|---|
| Contact frequency | Stiffness of overlap correction. Capped at an eighth of the substep rate (30 Hz) for stability. |
| Damping ratio | How much overlap correction is damped; Box2D's default of 10 is heavily overdamped. |
| Overlap push speed | The fastest overlap correction may separate bodies. |

Overlap beyond the slop, from a spawn, a placement or a hard shove, is pushed
apart at the spring's rate (about 9/s at the defaults), capped at the push speed.
A 0.05 m/s floor lets it arrive in finite time instead of approaching the slop
forever. Relaxation removes the push, so correction adds no momentum. In a
coincident pile, one relaxation sweep cannot fully cancel coupled push-outs:
bodies drift apart below the push speed, then passive drag stops them.

Each contact's warm-start force is physical state. It is hashed, reported as
`contact_forces`, and retired when either body is removed or its wall's scene
is evicted. A removed body's forces are filtered immediately and dropped at the
next step, so a mass death costs one pass rather than one rebuild per body.

## Movement and pushing

Game-owned [behavior actions](behaviors.md) choose target velocity and acceleration.
The [shared motor](../crates/sim/src/pass/motion.rs) computes one change per actor
per tick: 10% drag, then bounded acceleration toward the target. That whole
change ramps in across the substeps like a force, so a blocked motor leans on
its contacts steadily instead of striking them once per tick. A tick therefore
covers (v + ⅝ change) × dt, and a steady motor travels exactly at its requested
speed. Braking reserves the ⅜ of its current speed that the ramp still carries,
so arrival stops at, not inside, the stop distance.

Contacts exchange physical momentum, so faster actors push slower ones without
an attack. Knockback survives steering and decays gradually. A Move action
retains its motor until the body rests inside its arrival radius. Player
controls use the same bounded motor; tuning chooses acceleration and mass.

Horde animation derives walk/run/sprint cadence from actual displacement and
shares bounded poses. Its visual choice never drives gameplay.

## Fast motion

A tick splits into up to eight collision passes, so no body travels more than
0.16 m per pass. Speculative reach covers two bodies closing at that speed, which
keeps every possible contact inside neighbouring grid cells. A body struck to
high speed mid-tick therefore still finds the next body in its path within the
tick; a slam at the default 72 kg m/s carries through a row without crossing it.

Beyond eight passes, about 77 m/s, a continuous sweep slides bodies along walls
and the arena edge and stops them at panels on their path; the next pass removes their normal
speed. Body-to-body tunnelling is possible only at those speeds. Like Box2D's
rotation limit, a panel's spin is capped so its corner never outruns one pass.

## Measuring and limits

Start **Crowd stress — 10,000 pursuing actors** and wait for compression before
following [performance telemetry](performance.md), or profile the benchmark crowd
headlessly. On an M4, the compressed 10,000-body crowd costs about 3.7 ms per tick
on one thread: about 29,000 contact rows, one collision pass, four substeps.

Dense coincident piles still produce every pair in a cell; there is no contact
cap. Formations refuse spacing below a body diameter, so only explicit
placements or queued births can stack bodies. Overlap correction disperses a
pile at the push speed. 900 coincident bodies produce about 400,000 contact rows
on their worst tick (36 ms); the pile disperses within half a second.
