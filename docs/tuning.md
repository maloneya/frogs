# Runtime tuning

Open **Tuning** in [ARPG Companion](companion.md). Select a type, enter a numeric
value and press Return; Escape discards text. Steppers apply immediately, and a
field's Reset restores its built-in value. Stars mark differences from saved values.

**Save tuning** writes accepted values to `tuning.ron` in the game's working
directory. Unsubmitted text is not saved. **Revert saved** restores the last
successfully loaded/saved values. **Reset defaults** changes live values without
writing the file. **Restart scene** repeats the cached layout with current tuning.
Scene files do not contain these type-wide settings; see the
[editor state model](scene-editor.md#build-play-adjust).

[Sim's catalog](../crates/sim/src/tuning.rs) owns keys, units, ranges, defaults and
validation. Swift and the harness consume that catalog rather than copying it.
The Contacts group sets the global [contact response](crowd-collision.md#contact-response):
frequency, damping ratio and overlap push speed. These settings apply to every
contacting body; actors and gameplay roles do not override them. The defaults are
Box2D's: 30 Hz, damping ratio 10 and 3 m/s. They govern how quickly overlap is
corrected and how far heavy loads compress, not how much load a crowd transmits.

Movement and contact material use accepted settings on the next tick. Movement
speed and acceleration govern motor effort; external momentum remains physical. Melee changes affect subsequent swings; an active swing
keeps its captured impulse. Scene starts, restart, additive loads and eviction
preserve tuning. Animation retains its bounded buckets and authored reference speeds.

Invalid edits preserve live state and queued work. Accepted values enter simulation
hashes and emit `TuningChanged` between ticks. Files require the complete catalog;
failed saves preserve the previous file/baseline. Invalid startup content leaves
defaults active and reports the error. Use `ARPG_TUNING_FILE` for an isolated experiment.

## Automation and evidence

The [harness](../.claude/skills/playtest/SKILL.md) supports:

```text
tuning set player_speed 6
tuning save
tuning revert
tuning defaults
tuning restart
```

`state.sim.tuning` reports accepted values; `state.tuning` reports persistence;
`state.companion` reports command status/errors. See [native checks and capture](companion.md#checks-and-capture)
for testing the UI.

Use the [scenario guide](../.claude/skills/scenario/SKILL.md) for timed tuning edits.