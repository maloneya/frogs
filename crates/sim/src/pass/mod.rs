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
//! 1. [`remember`] — snapshot every body's position *before* anything moves.
//!    Must be first, and this is the one ordering mistake with a visible
//!    symptom rather than a wrong number: a stale snapshot streaks every body
//!    on screen from where it used to be.
//! 2. [`walk`] — integrate the player's input.
//! 3. [`separate::crowd`] — push overlapping *enemies* apart. **After**
//!    walking, so it resolves where things actually are this tick rather than
//!    where they were.
//! 4. [`separate::player`] — push the horde off the player. **After**
//!    `crowd`, and that order is load-bearing: one Gauss-Seidel sweep leaves
//!    residual overlap, and whichever runs last is the one whose result
//!    survives the tick. Last here means nothing ends a tick standing inside
//!    the character the camera is centred on; the residual lands on
//!    enemy-against-enemy instead, where it is invisible.
//! 5. [`contain`] — put everything back inside the arena. **After** separating,
//!    because clamping first would let a contact shove a body through the wall
//!    and leave it there until something else happened to touch it.
//! 6. [`face`] — turn the body toward its direction of travel. Last because
//!    nothing else reads facing yet; when an attack hitbox does, it will be
//!    oriented by the facing this pass leaves behind, and that will make the
//!    position of this line in the list load-bearing.
//!
//! ## What is not yet enforced
//!
//! The player is a single struct rather than a row in the horde's storage, so
//! the passes below take its individual fields — `&mut Vec2`, `&mut f32` —
//! where the horde gets a real slice. That is the honest limit today: the horde
//! half of every signature is checked by the compiler, the player half is
//! checked by reading it. Both become slices when the player joins SoA storage
//! and `EntityId` exists (roadmap chunk 4).

pub(crate) mod attack;
pub(crate) mod contain;
pub(crate) mod face;
pub(crate) mod remember;
pub(crate) mod seek;
pub(crate) mod separate;
pub(crate) mod walk;
