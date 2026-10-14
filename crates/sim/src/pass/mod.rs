//! Simulation passes. [`crate::World::step`] defines their order.

pub(crate) mod attack;
/// Channel admission suppresses powered walk before movement, leaving carried
/// physics intact. Attack resolves its damage pulses after final positions and
/// facing, with continuous impulses, before defeated bodies are removed. Release ends it on that tick.
pub(crate) mod beam;
pub(crate) mod face;
pub(crate) mod health;
pub(crate) mod interact;
pub(crate) mod remember;
pub(crate) mod source;
pub(crate) mod topple;
pub(crate) mod spawn;

pub(crate) mod motion;
