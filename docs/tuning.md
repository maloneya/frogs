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
validation. Swift displays that catalog rather than copying it. The derived
monster motor speed ceiling shows when acceleration limits the requested speed.
Settings apply to existing and future members of a type, not individual bodies.

Movement uses accepted settings on the next tick. Zero monster speed does not
erase carried momentum. Melee changes affect subsequent swings; an active swing
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