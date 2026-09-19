//! One scene-owned melee attacker and the player's first health rule.
//!
//! This deliberately has no wind-up, animation, knockback, defeat, or crowd
//! coordination. A registered seeker that reaches the player's collision
//! boundary removes one health, then waits one second before it may do so
//! again. The player keeps moving normally because this state has no route to
//! input, position, facing, or carried motion.

use arpg_core::Report;
use arpg_sim::{EntityId, Fnv, MeleeView, SceneId, TICK_HZ, TraceSink};
use serde::Deserialize;
use crate::encounter::EncounterEvent;

/// Health available at the beginning of every run.
pub const PLAYER_MAX_HEALTH: u8 = 5;

/// Exactly one second between damage ticks at the fixed simulation rate.
const DAMAGE_INTERVAL_TICKS: u32 = TICK_HZ;

/// Binds the scene's one melee attacker to an explicit seeking body.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyAttackSpec {
    /// Index in the engine scene's explicit bodies.
    pub body: usize,
}

struct Attacker {
    owner: SceneId,
    body: EntityId,
    cooldown: u32,
}

pub(crate) struct EnemyAttacks {
    attacker: Option<Attacker>,
    player_health: u8,
}

impl Default for EnemyAttacks {
    fn default() -> Self {
        Self {
            attacker: None,
            player_health: PLAYER_MAX_HEALTH,
        }
    }
}

impl EnemyAttacks {
    pub(crate) fn validate_add(
        &self,
        spec: Option<EnemyAttackSpec>,
        bodies: &[arpg_sim::Placed],
    ) -> Result<(), crate::SceneError> {
        let Some(spec) = spec else { return Ok(()) };
        if self.attacker.is_some() {
            return Err(crate::SceneError::EnemyAttack(
                "only one enemy attacker may be live",
            ));
        }
        if !bodies.get(spec.body).is_some_and(|body| body.what.seeks()) {
            return Err(crate::SceneError::EnemyAttack(
                "enemy attacker must bind an explicit seeking body",
            ));
        }
        Ok(())
    }

    pub(crate) fn install(
        &mut self,
        owner: SceneId,
        spec: Option<EnemyAttackSpec>,
        bodies: &[EntityId],
    ) {
        if let Some(spec) = spec {
            debug_assert!(self.attacker.is_none());
            self.attacker = Some(Attacker {
                owner,
                body: bodies[spec.body],
                cooldown: 0,
            });
        }
    }

    pub(crate) fn evict(&mut self, owner: SceneId) {
        if self
            .attacker
            .as_ref()
            .is_some_and(|attacker| attacker.owner == owner)
        {
            self.attacker = None;
        }
    }

    pub(crate) fn remove_body(&mut self, body: EntityId) {
        if self.attacker.as_ref().is_some_and(|attacker| attacker.body == body) {
            self.attacker = None;
        }
    }

    pub(crate) fn player_health(&self) -> u8 {
        self.player_health
    }

    pub(crate) fn is_pristine(&self) -> bool {
        self.attacker.is_none() && self.player_health == PLAYER_MAX_HEALTH
    }

    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(usize::from(self.player_health));
        out.usize(usize::from(self.attacker.is_some()));
        if let Some(attacker) = &self.attacker {
            attacker.owner.hash_into(out);
            attacker.body.hash_into(out);
            out.usize(attacker.cooldown as usize);
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.object("player_health", |out| {
            out.int("remaining", u64::from(self.player_health));
            out.int("maximum", u64::from(PLAYER_MAX_HEALTH));
        });
        out.object("enemy_attack", |out| {
            out.bool("active", self.attacker.is_some());
            if let Some(attacker) = &self.attacker {
                out.text("body", &attacker.body.to_string());
                out.int("cooldown_ticks", u64::from(attacker.cooldown));
            }
        });
    }
}

/// Resolve damage after the engine tick, from its final authoritative positions.
/// A body defeated during that tick is already gone and cannot trade a late hit.
pub(crate) fn advance(
    attacks: &mut EnemyAttacks,
    view: MeleeView<'_>,
    mut trace: TraceSink<'_, EncounterEvent>,
) {
    let Some(attacker) = attacks.attacker.as_mut() else {
        return;
    };
    let Some(reached_player) = view.reached_player(attacker.body) else {
        attacks.attacker = None;
        return;
    };
    attacker.cooldown = attacker.cooldown.saturating_sub(1);
    if attacks.player_health == 0 || attacker.cooldown != 0 || !reached_player {
        return;
    }
    attacks.player_health -= 1;
    attacker.cooldown = DAMAGE_INTERVAL_TICKS;
    trace.emit(EncounterEvent::PlayerDamaged {
        attacker: attacker.body,
        remaining: attacks.player_health,
    });
}
