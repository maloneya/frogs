# Audio sources

The four sampled cues are by **Kenney (www.kenney.nl)**, released under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). The upstream license
notices are preserved in [licenses/kenney-rpg.txt](licenses/kenney-rpg.txt) and
[licenses/kenney-impact.txt](licenses/kenney-impact.txt).

Downloaded 2026-09-27 from [RPG Audio](https://kenney.nl/assets/rpg-audio) and
[Impact Sounds](https://kenney.nl/assets/impact-sounds).

| Runtime WAV | Pack / original file | Retained interval (seconds) | Peak amplitude |
| --- | --- | --- | --- |
| `cleave.wav` | RPG Audio / `Audio/knifeSlice.ogg` | 0.077–0.480 | 0.50 |
| `slam.wav` | Impact Sounds / `Audio/impactPunch_heavy_000.ogg` | 0–0.560 | 0.65 |
| `impact.wav` | RPG Audio / `Audio/chop.ogg` | Entire clip | 0.50 |
| `footstep.wav` | Impact Sounds / `Audio/footstep_concrete_000.ogg` | Entire clip | 0.40 |

Processing: decode OGG with Python SoundFile, average stereo channels to mono,
crop at rounded sample boundaries, apply linear 2 ms fade-in and 10 ms fade-out
(including zero at each outer endpoint), then scale to the listed peak and write
16-bit PCM WAV at the original rate (RPG: 48 kHz; Impact: 44.1 kHz). No pitch or
timing changes. Cleave's leading silence is removed to align with activation;
short tails keep repeated contacts and footsteps distinct. Existing cue and
master gains in `crates/app/src/audio.rs` and `audio/playback.rs` apply on top.

Archive SHA-256:

- `kenney_rpg-audio.zip`: `6dbeaf8544da958d8f2adcb4a4a4b76c1ade34a05f8ab9edccd327da7375f38b`
- `kenney_impact-sounds.zip`: `029d734af1582474edf3a694d1b0cebc97c1c152f2f39fa34d4c2bafc5de77f8`

Original OGG SHA-256:

- `knifeSlice.ogg`: `4cd96dc630bed9840c15f1dd2306da2cc56a4da26a5d3f1a03c5a7265ac5e54f`
- `impactPunch_heavy_000.ogg`: `b33a8f14068aec24ec69ba85e5e87fdc41228975f6a1a3e44a6e7d6fc3d9f8d8`
- `chop.ogg`: `d00c2b3c9fff07e376145c8c8c45c90e5084ec192f6ce0387db233f7b86f1486`
- `footstep_concrete_000.ogg`: `d7267e183067757c92c169de2a379abea592cfca6b39bb2e8feea15221ad79fe`

`hurt.wav`, `beam.wav` and `ambience.wav` are original synthesized prototypes.
Regenerate only those three with `python3 scripts/generate_audio.py`.
