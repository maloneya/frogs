# Audio

The app observes each completed Game tick, including ticks between redraws.
Kira owns decoding, sample-rate conversion, playback handles, mixing and the
macOS output device. Sim, Game and gfx have no audio dependency. Audio never
reads the diagnostic trace or changes gameplay state or hashes.

The current presentation is deliberately small:

- Cleave and Slam play their committed profile's cue when the active window
  opens. The first contact plays one impact per swing, regardless of enemy count.
  Misses stay silent apart from the swing. There are no individual horde voices.
- A held beam has one looping hum. Release or an accepted attack selection
  change stops it with a short fade. Each damage pulse that hits plays one impact cue, regardless of target count;
  misses produce only the hum.
- Footsteps play every 0.3 seconds of consecutive walking progress. Movement
  intent and actual progress are both required; idle knockback, pushing into a
  wall and beam channeling do not produce steps. This is an approximate cadence,
  not foot-contact markers from animation. Carrying motion while also walking
  can affect the progress gate.
- Player health loss produces one hurt cue per completed tick.
- One stereo ambience bed plays while any scene is loaded. Additive scenes
  share it; evicting the last scene fades it out. A successful start/restart
  clears voices and observations before starting the replacement ambience.
  Rejected loads preserve playback. This is a common prototype bed, not
  authored per-scene ambience or positioned environmental emitters.

Player sounds are centred. All seven WAVs are decoded once at startup and
shared between voices. The app bounds one-shots at 16; Kira bounds the full
track at 32, leaving room for two loops and short fades during replacement.
Excess playback is refused and counted. Mixing stays on Kira's audio thread.

The cue bank and volume tuning live in `crates/app/src/audio.rs`; device and
voice lifetime live in `audio/playback.rs`. The adapter accepts typed cues and
desired loop state, so repeated observations cannot restart a loop. There is
no parallel custom mixer or alternate audio backend in production.

`state.audio` reports device availability, startup failure, per-run one-shot
cue counts, accepted/refused playback requests, active one-shots and loop
handles. Cue counts are presentation decisions even without a device;
`started` counts Kira-accepted playback. Loop flags describe owned playback
handles, not proof of audible speaker output. Device initialization failure
is logged and reported, and gameplay continues silently.

## Assets and checks

The WAVs in `assets/audio` are original synthesized prototype sounds generated
by `python3 scripts/generate_audio.py`, using no external recordings. Replace
them in place to change the sound design. These are not final combat Foley.

Tests drive ordinary Game ticks to check cue timing, committed attacks, misses,
multi-target hits, walking, knockback, channel suppression and restart. Kira's
mock device checks real WAV decoding, one-shot completion, persistent loops,
stop fades and voice limits without requiring speakers.

For live checks, use the [playtest guide](../.claude/skills/playtest/SKILL.md).
Inspect `state.audio`, `hold d 600`, `tap 1`, `hold space 500`, then
`scene restart`. Walking should produce steps; a held beam should own one hum
and release it; restart should reset counts and retain just the fresh ambience.
Judge clip quality, loudness and exact footstep feel by listening.
