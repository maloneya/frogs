//! The simulation schedule: one module per named pass.
//!
//! A behaviour here is a *pass*, not a block appended to `World::step`. The
//! reason is not tidiness. Code written into `step` carries its ordering in
//! control flow, where nothing can read it or stop the next edit getting it
//! wrong; and a function taking `&mut World` can touch anything, so "this pass
//! only reads positions" is a claim rather than a fact. Both are avoidable for
//! free — a pass takes the data it declares, and the borrow checker enforces
//! the declaration at layer 0.
//!
//! ## The order, and why each adjacency is what it is
//!
//! This list must match the body of `World::step`. It is the readable copy of
//! an order that would otherwise live only in control flow.
//!
//! Scene load/evict complete between ticks, through World doors unavailable to
//! passes. Loading therefore exposes all authored content to the next source
//! evaluation; eviction cancels sources and their queued work before it. Source
//! requests carry their scene owner and the spawn drain records ownership at
//! grant time. There is no background or partial scene instantiation yet.
//!
//! 1. [`source::trigger`] — decide which sources fire, and ask. **Before the
//!    drain**, so a source that fires on a tick produces a body that lives the
//!    whole of it; and separate from the drain, because deciding and performing
//!    have different privileges — this pass is handed no storage at all, so the
//!    code that decides bodies exist cannot make one.
//! 2. [`spawn::drain`] — grant everything asked for since the last tick.
//!    **Before anything holds a row**, and structurally so: it is the only pass
//!    that changes what exists, so running it here makes the horde's length
//!    constant for the rest of the tick and a row index taken by one pass still
//!    that body when the next runs. It also means a new body exists for a
//!    *whole* tick rather than part of one — no first-frame special case, which
//!    in a fixed-step loop is just a bug with a schedule.
//! 3. [`remember`] — snapshot every body's position *before* anything moves.
//!    This is the one ordering mistake with a visible symptom rather than a
//!    wrong number: a stale snapshot streaks every body on screen from where
//!    it used to be.
//! 4. [`walk`] — integrate the player's input.
//! 5. [`seek`] — walk the chasers. **After** `walk`, so they steer at where the
//!    player is now rather than where it stood at the start of the tick, and
//!    **before** the solvers, so the pile-up chasing creates is what they
//!    resolve.
//! 6. [`motion::integrate`] — add carried motion after powered locomotion,
//!    before testing contacts. Neither steering nor overlap correction writes
//!    carried velocity, so neither can erase or manufacture a blow.
//! 7. [`separate::crowd`] — repair enemy overlaps and exchange momentum.
//! 8. [`separate::player`] — do the same for the player against the horde.
//!    After crowd resolution, preserving the existing player-space priority.
//!    Both responses use the same physics store and mass properties.
//! 9. [`contain`] — clamp positions and cancel outward wall velocity. After
//!    separation, so a correction cannot leave a body outside the arena.
//! 10. [`motion::settle`] — damp carried velocity and snap its small tail to
//!     rest. After contact response, before a new attack impulse.
//! 11. [`face`] — turn toward the requested direction before placing a hitbox.
//! 12. [`attack`] — test final positions, apply an impulse and subtract one hit
//!     point once per target per swing. Velocity changes now; displacement
//!     begins on the next tick.
//! 13. [`health::remove_defeated`] — remove bodies reduced to zero. **Last**, so
//!     its dense-row swaps cannot invalidate an index another pass will use.
//!     Surviving bodies retain their new velocity and remain in every solver.
//!
//! The player occupies row zero in the shared body store. Its facing and
//! attack state are player-only; physics membership and payload use the same
//! identity and mechanisms as every other physical body.

pub(crate) mod attack;
pub(crate) mod contain;
pub(crate) mod face;
pub(crate) mod health;
pub(crate) mod remember;
pub(crate) mod seek;
pub(crate) mod separate;
pub(crate) mod source;
pub(crate) mod spawn;
pub(crate) mod walk;

pub(crate) mod motion;
