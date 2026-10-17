//! Tick-driven sound presentation. Kira owns decoding, voices, mixing and the device.
//! No diagnostic trace is read and no audio state can write back into Game.

mod playback;
#[cfg(test)]
mod tests;

use arpg_core::{Intent, Report};
use arpg_game::{Game, PLAYER_MAX_HEALTH};
use arpg_sim::{Alpha, AttackPhase, TICK_HZ};
use arpg_game::{AttackProfile};
use glam::Vec3Swizzles;

use playback::Playback;

/// Audio content and its tuning have one owner. Loops use the same decoded bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cue { Cleave, Slam, Impact, Footstep, Hurt, Beam, Ambience }

impl Cue {
    const ALL: [Self; 7] = [Self::Cleave, Self::Slam, Self::Impact, Self::Footstep,
        Self::Hurt, Self::Beam, Self::Ambience];

    fn name(self) -> &'static str {
        match self {
            Self::Cleave => "cleave", Self::Slam => "slam", Self::Impact => "impact",
            Self::Footstep => "footstep", Self::Hurt => "hurt", Self::Beam => "beam",
            Self::Ambience => "ambience",
        }
    }

    fn decibels(self) -> f32 {
        match self {
            Self::Cleave | Self::Slam | Self::Impact => -6.0,
            Self::Hurt => -3.0,
            Self::Footstep => -15.0,
            Self::Beam => -3.0,
            Self::Ambience => -12.0,
        }
    }
}

/// Approximate walking cadence, independent of redraws; not animation markers.
const STEP_TICKS: u32 = TICK_HZ * 3 / 10;

struct Observations {
    tick: Option<u64>,
    contact_played: bool,
    walking_ticks: u32,
    health: u8,
}

impl Default for Observations {
    fn default() -> Self {
        Self { tick: None, contact_played: false, walking_ticks: 0,
            health: PLAYER_MAX_HEALTH }
    }
}

impl Observations {
    /// Read after every Game step, including catch-up ticks. Duplicate reads do nothing.
    fn observe(&mut self, game: &Game, intent: Intent, mut emit: impl FnMut(Cue)) {
        if self.tick == Some(game.tick()) { return; }
        self.tick = Some(game.tick());
        let player = game.player_presentation(Alpha::ONE);
        let attack = player.attack();
        let active = attack.phase == AttackPhase::Active && !attack.channeling;
        if !active { self.contact_played = false; }
        if active && attack.swing_resolved.is_some_and(|swing| attack.elapsed == swing.startup()) {
            match attack.swing_profile {
                Some(AttackProfile::Cleave) => emit(Cue::Cleave),
                Some(AttackProfile::Slam) => emit(Cue::Slam),
                _ => {}
            }
        }
        if active && attack.struck > 0 && !self.contact_played {
            emit(Cue::Impact);
            self.contact_played = true;
        }
        if !game.beam_hits().is_empty() { emit(Cue::Impact); }
        let health = game.player_health();
        if health < self.health { emit(Cue::Hurt); }
        self.health = health;

        // Intent gates involuntary displacement. Actual progress gates a held
        // key against a wall. Channeling suppresses powered walking in sim.
        let progress = player.displacement().dot(intent.move_dir().as_vec3().xz());
        if progress > 0.0001 && !attack.channeling {
            self.walking_ticks += 1;
            if self.walking_ticks == STEP_TICKS {
                self.walking_ticks = 0;
                emit(Cue::Footstep);
            }
        } else {
            self.walking_ticks = 0;
        }
    }
}

/// App-owned lifetime: survives scene changes; voices and observations reset per run.
#[derive(Default)]
pub(crate) struct Audio {
    playback: Option<Playback>,
    observations: Observations,
    cues: [u64; Cue::ALL.len()],
    error: Option<String>,
    muted: bool,
}

impl Audio {
    /// Device failure is visible in logs and harness state, and cannot stop gameplay.
    pub(crate) fn initialize(&mut self, game: &Game, muted: bool) {
        self.muted = muted;
        match Playback::open(muted) {
            Ok(playback) => self.playback = Some(playback),
            Err(error) => {
                log::warn!("audio unavailable: {error}");
                self.error = Some(error);
            }
        }
        self.reset(game);
    }

    /// Mute the final mix while voices and tick observations continue normally.
    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(playback) = &mut self.playback { playback.set_muted(muted); }
    }

    /// Called only after a successful scene replacement. Rejected loads keep audio intact.
    pub(crate) fn reset(&mut self, game: &Game) {
        self.observations = Observations { tick: Some(game.tick()), health: game.player_health(), ..Default::default() };
        self.cues = [0; Cue::ALL.len()];
        if let Some(playback) = &mut self.playback { playback.clear(); }
        self.sync_loops(game);
    }

    pub(crate) fn observe(&mut self, game: &Game, intent: Intent) {
        let Self { playback, observations, cues, .. } = self;
        observations.observe(game, intent, |cue| {
            cues[cue as usize] += 1;
            if let Some(playback) = playback { playback.one_shot(cue); }
        });
        self.sync_loops(game);
    }

    /// Reconcile persistent state even on redraws with no tick (e.g. scene eviction).
    pub(crate) fn sync_loops(&mut self, game: &Game) {
        if let Some(playback) = &mut self.playback {
            playback.loops(game.attack_status().channeling, game.scene_count() != 0);
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        out.bool("available", self.playback.is_some());
        out.bool("muted", self.muted);
        if let Some(error) = &self.error { out.text("error", error); }
        out.object("cues", |out| {
            for cue in [Cue::Cleave, Cue::Slam, Cue::Impact, Cue::Footstep, Cue::Hurt] {
                out.int(cue.name(), self.cues[cue as usize]);
            }
        });
        if let Some(playback) = &self.playback { playback.report(out); }
    }
}
