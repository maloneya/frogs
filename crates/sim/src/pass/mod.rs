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
//! 6. [`separate::crowd`] — push overlapping *enemies* apart.
//! 7. [`separate::player`] — push the horde off the player. **After** `crowd`,
//!    and that order is load-bearing: one Gauss-Seidel sweep leaves residual
//!    overlap, and whichever runs last is the one whose result survives the
//!    tick. Last here means nothing ends a tick standing inside the character
//!    the camera is centred on; the residual lands on enemy-against-enemy
//!    instead, where it is invisible.
//! 8. [`contain`] — put everything back inside the arena. **After** separating,
//!    because clamping first would let a contact shove a body through the wall
//!    and leave it there until something else happened to touch it.
//! 9. [`face`] — turn the body toward its direction of travel. **Before**
//!    `attack`, which is what makes the hitbox swing where the character ends
//!    the tick pointing.
//! 10. [`attack`] — advance the swing and test the hitbox. **Last**, so it is
//!     tested against where the bodies actually ended up: after seeking, after
//!     both solvers, after the wall.
//!
//! ## What is not yet enforced
//!
//! The player is a single struct rather than a row in the horde's storage, so
//! the passes below take its individual fields — `&mut Vec2`, `&mut f32` —
//! where the horde gets a real slice. That is the honest limit today: the horde
//! half of every signature is checked by the compiler, the player half is
//! checked by reading it. Both become slices when the player joins SoA storage
//! (roadmap chunk 4).

pub(crate) mod attack;
pub(crate) mod contain;
pub(crate) mod face;
pub(crate) mod remember;
pub(crate) mod seek;
pub(crate) mod separate;
pub(crate) mod source;
pub(crate) mod spawn;
pub(crate) mod walk;
