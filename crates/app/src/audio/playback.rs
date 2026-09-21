//! The only Kira-facing adapter; no entities or mutable game access cross it.

use std::time::Duration;

use arpg_core::Report;
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween, backend::Backend,
    sound::{PlaybackState, static_sound::{StaticSoundData, StaticSoundHandle}},
    track::MainTrackBuilder};

use super::Cue;

const ONE_SHOTS: usize = 16;
// Headroom lets old voices finish short stop fades during scene replacement.
const VOICES: usize = 32;

fn fade(milliseconds: u64) -> Tween {
    Tween { duration: Duration::from_millis(milliseconds), ..Default::default() }
}

fn bank() -> Result<[StaticSoundData; Cue::ALL.len()], String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio");
    let mut sounds = Vec::with_capacity(Cue::ALL.len());
    for cue in Cue::ALL {
        let path = root.join(format!("{}.wav", cue.name()));
        let sound = StaticSoundData::from_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if sound.frames.is_empty() || sound.sample_rate == 0 || sound.frames.iter().any(|frame|
            !frame.left.is_finite() || !frame.right.is_finite()) {
            return Err(format!("{}: empty or invalid samples", path.display()));
        }
        sounds.push(sound.volume(cue.decibels()));
    }
    // Complete decoding precedes device creation and any playback.
    Ok(sounds.try_into().expect("one clip for each cue"))
}

pub(super) struct Playback<B: Backend = DefaultBackend> {
    manager: AudioManager<B>,
    clips: [StaticSoundData; Cue::ALL.len()],
    shots: Vec<StaticSoundHandle>,
    beam: Option<StaticSoundHandle>,
    ambience: Option<StaticSoundHandle>,
    started: u64,
    refused: u64,
}

impl Playback {
    pub(super) fn open() -> Result<Self, String> {
        let clips = bank()?;
        let manager = AudioManager::<DefaultBackend>::new(settings()).map_err(|error| error.to_string())?;
        Ok(Self::new(manager, clips))
    }
}

fn settings<B: Backend>() -> AudioManagerSettings<B> where B::Settings: Default {
    AudioManagerSettings {
        main_track_builder: MainTrackBuilder::new().sound_capacity(VOICES).volume(-6.0),
        ..Default::default()
    }
}

impl<B: Backend> Playback<B> {
    fn new(manager: AudioManager<B>, clips: [StaticSoundData; Cue::ALL.len()]) -> Self {
        Self { manager, clips, shots: Vec::with_capacity(ONE_SHOTS), beam: None,
            ambience: None, started: 0, refused: 0 }
    }

    pub(super) fn one_shot(&mut self, cue: Cue) {
        self.shots.retain(|voice| voice.state() != PlaybackState::Stopped);
        if self.shots.len() == ONE_SHOTS { self.refused += 1; return; }
        match self.manager.play(self.clips[cue as usize].clone()) {
            Ok(voice) => { self.shots.push(voice); self.started += 1; }
            Err(_) => self.refused += 1,
        }
    }

    pub(super) fn loops(&mut self, beam: bool, ambience: bool) {
        for (wanted, cue, slot) in [(beam, Cue::Beam, &mut self.beam),
            (ambience, Cue::Ambience, &mut self.ambience)] {
            if wanted && slot.is_none() {
                let sound = self.clips[cue as usize].loop_region(..).fade_in_tween(fade(120));
                match self.manager.play(sound) {
                    Ok(voice) => { *slot = Some(voice); self.started += 1; }
                    Err(_) => self.refused += 1,
                }
            } else if !wanted && let Some(mut voice) = slot.take() {
                voice.stop(fade(60));
            }
        }
    }

    pub(super) fn clear(&mut self) {
        for mut voice in self.shots.drain(..) { voice.stop(fade(20)); }
        if let Some(mut voice) = self.beam.take() { voice.stop(fade(20)); }
        if let Some(mut voice) = self.ambience.take() { voice.stop(fade(20)); }
        self.started = 0;
        self.refused = 0;
    }

    pub(super) fn report(&self, out: &mut Report) {
        out.int("started", self.started);
        out.int("refused", self.refused);
        out.int("one_shots", self.shots.iter().filter(|voice| voice.state() != PlaybackState::Stopped).count() as u64);
        out.bool("beam", self.beam.is_some());
        out.bool("ambience", self.ambience.is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::backend::mock::{MockBackend, MockBackendSettings};

    fn player() -> Playback<MockBackend> {
        let mut settings = settings::<MockBackend>();
        settings.backend_settings = MockBackendSettings { sample_rate: 24_000 };
        Playback::new(AudioManager::new(settings).unwrap(), bank().unwrap())
    }

    fn advance(player: &mut Playback<MockBackend>, blocks: usize) {
        for _ in 0..blocks {
            player.manager.backend_mut().on_start_processing();
            player.manager.backend_mut().process();
        }
    }

    #[test]
    fn real_clips_finish_but_loops_survive_until_stopped() {
        let mut player = player();
        player.one_shot(Cue::Footstep);
        player.loops(true, true);
        for _ in 0..20 { player.loops(true, true); }
        assert_eq!(player.started, 3, "observing again must not layer loops");
        advance(&mut player, 250); // 1.33 seconds: longer than the beam's loop.
        assert_eq!(player.shots[0].state(), PlaybackState::Stopped);
        assert_eq!(player.beam.as_ref().unwrap().state(), PlaybackState::Playing);
        assert_eq!(player.ambience.as_ref().unwrap().state(), PlaybackState::Playing);
        let mut beam = player.beam.take().unwrap();
        beam.stop(fade(60));
        advance(&mut player, 30);
        assert_eq!(beam.state(), PlaybackState::Stopped);
        player.clear();
        advance(&mut player, 30);
        assert!(player.beam.is_none() && player.ambience.is_none() && player.shots.is_empty());
        assert_eq!(player.manager.main_track().num_sounds(), 0);
    }

    #[test]
    fn excess_one_shots_cannot_displace_loops_and_finished_slots_are_reused() {
        let mut player = player();
        for _ in 0..ONE_SHOTS + 3 { player.one_shot(Cue::Impact); }
        assert_eq!(player.started, ONE_SHOTS as u64);
        assert_eq!(player.refused, 3);
        player.loops(true, true);
        assert!(player.beam.is_some() && player.ambience.is_some());
        advance(&mut player, 60);
        player.one_shot(Cue::Footstep);
        assert_eq!(player.shots.len(), 1);
        assert_eq!(player.refused, 3);
        player.loops(false, false);
        assert!(player.beam.is_none() && player.ambience.is_none());
        advance(&mut player, 60);
        assert_eq!(player.manager.main_track().num_sounds(), 0);
    }
}
