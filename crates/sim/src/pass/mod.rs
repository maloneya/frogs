//! Simulation passes. [`crate::PreparedStep::advance`] defines their order.

pub(crate) mod health;
pub(crate) mod interact;
pub(crate) mod remember;
pub(crate) mod source;
pub(crate) mod topple;
pub(crate) mod spawn;

pub(crate) mod motion;
