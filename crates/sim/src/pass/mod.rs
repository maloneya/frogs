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
//!    code that decides bodies exist cannot make one. Disabled sources skip
//!    cadence and condition evaluation, preserving countdown and ring progress.
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
//! 5. [`seek`] — choose staggered ground gaits and ask the physics motor to
//!    accelerate chasers. Reads the player's post-walk position. The motor
//!    applies drag once and bounds its velocity correction; a blow therefore
//!    takes time to overcome. It cannot grant behaviours or change positions.
//! 6. [`separate::prevent_crossings`] predicts fast physical crossings and
//!    exchanges momentum before integration, at most one tick early. Walls
//!    shield pairs. No positions or membership change in this ordered sweep.
//!    [`motion::integrate`] — integrate physical velocity once before contacts.
//!    Enemy running and knockback share velocity, so both transmit momentum.
//!    Player controls still move directly; their additional knockback integrates
//!    here. Overlap correction never manufactures velocity.
//!    [`walls::resolve`] then sweeps circles from their remembered positions to
//!    stop motion at authored rectangles before bodies exchange momentum.
//! 7. [`separate::crowd`] — repair enemy overlaps and exchange momentum.
//!    Rebuild its grid from post-motion rows, then update cell membership as
//!    each correction lands. Candidate order preserves the original pair sweep.
//! 8. [`separate::player`] — do the same for the player against the horde.
//!    After crowd resolution, preserving the existing player-space priority.
//!    Both responses use the same physics store and mass properties.
//! 9. [`contain`] — clamp positions and cancel outward wall velocity. After
//!    separation, so a correction cannot leave a body outside the arena.
//!    [`walls::resolve`] runs again after containment: crowd/player corrections
//!    cannot push a circle through a rectangle. Both checks cancel only velocity
//!    into the contact face; wall contacts are aggregated once per pass.
//! 10. [`motion::settle`] — damp passive velocity and snap its small tail to
//!     rest. Motors already applied drag; consume their tick markers here.
//!     After contact response, before a new attack impulse.
//! 11. [`face`] — follow independent aim before placing a hitbox; snap on an accepted
//!     swing and hold that facing through recovery. Channels follow aim each tick.
//! 12. [`attack`] — test final positions, apply an impulse and subtract resolved
//!     damage once per target per swing. Velocity changes now; displacement
//!     begins on the next tick. Channels share this boundary: every touching
//!     tick submits momentum, damage pulses record hit positions before removal.
//! 13. [`interact`] — activate one ready object using final positions. A press
//!     changes only interaction state; it cannot spawn or move bodies.
//! 14. [`health::remove_defeated`] — remove bodies reduced to zero. **Last**, so
//!     its dense-row swaps cannot invalidate an index another pass will use.
//!     Before removal, record each defeat and its source provenance in the
//!     lossless completed-tick outcomes consumed by Game. These are independent
//!     of diagnostic history. Surviving bodies retain their new velocity.
//!
//! The player occupies row zero in the shared body store. Its facing and
//! attack state are player-only; physics membership and payload use the same
//! identity and mechanisms as every other physical body.

pub(crate) mod attack;
/// Channel admission suppresses powered walk before movement, leaving carried
/// physics intact. Attack resolves its damage pulses after final positions and
/// facing, with continuous impulses, before defeated bodies are removed. Release ends it on that tick.
pub(crate) mod beam;
pub(crate) mod contain;
pub(crate) mod face;
pub(crate) mod health;
pub(crate) mod interact;
pub(crate) mod remember;
pub(crate) mod seek;
pub(crate) mod separate;
pub(crate) mod source;
pub(crate) mod spawn;
pub(crate) mod walk;

pub(crate) mod motion;

/// Sweep circles against rectangles before crowd contacts and after arena containment.
pub(crate) mod walls;
