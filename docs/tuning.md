# Runtime tuning

Tuning is a tab in the native [ARPG Companion](companion.md). Build it once from
the repository root:

```sh
python3 scripts/companion.py build
cargo run --release
```

Press **F4** in the game to open or focus **ARPG Companion**. Alternatively,
`python3 scripts/companion.py open` builds and opens it. Open **Tuning**, select a type in the sidebar,
edit a numeric field and press Return to apply it. Escape discards that draft.
Native steppers apply immediately; Reset beside a field restores its built-in
value. Closing the companion leaves the game running.

**Save tuning** writes accepted values to `tuning.ron` in the game's working
directory. Unsubmitted text is not saved. **Revert saved** restores the last
successfully loaded/saved values. **Reset defaults** restores built-in values
without overwriting the file. Stars mark differences from saved values.
**Restart encounter** repeats the cached scene with current tuning.

The first catalog covers player movement speed, monster walk/run/sprint speeds,
monster acceleration, and Cleave/Slam impulse. The authoritative catalog, units,
ranges and defaults are in [sim tuning](../crates/sim/src/tuning.rs). Swift has
no copy of that list: the game sends it with the accepted values. The panel
shows the derived monster motor speed ceiling; raise acceleration when exploring
speeds above that ceiling. These are type-wide settings, including existing
monsters, rather than edits to individual bodies.

Movement uses accepted settings on the next tick. Physical momentum still
applies, so zero monster speed does not instantly erase carried motion. Melee
changes affect subsequent swings; an in-flight swing keeps its captured impulse.
Additive loading, eviction, new scene starts and restart preserve tuning.
Animation keeps its bounded buckets and authored reference speeds.

Invalid edits leave live state and queued work untouched. Validation belongs to
sim, shared by files, harness and companion. Accepted tuning participates in
simulation hashes and emits `TuningChanged` events at the between-tick boundary.
Files must contain the complete catalog. Failed saves preserve the previous file
and saved baseline. An invalid startup file leaves built-in tuning active; the
companion displays the load error. Use `ARPG_TUNING_FILE=/path/to/tuning.ron`
for a separate experiment.

## Harness and verification

The opt-in [live harness](../.claude/skills/playtest/SKILL.md) remains independent:

```text
tuning set player_speed 6
tuning set slam_impulse 24
tuning save
tuning revert
tuning defaults
tuning restart
state
```

Field keys come from the owning catalog. `state.sim.tuning` reports accepted
values; `state.tuning` reports persistence. `state.companion` reports the latest
command status and any error. The harness has no
panel pointer, keyboard, capture, or window lifecycle commands.

`python3 scripts/companion.py test` tests the native model and builds the app bundle.
Full repository verification includes that check. For a self-capture of the
native tuning view, launch the built executable with
`ARPG_COMPANION_CAPTURE=/tmp/tuning.png target/"ARPG Companion.app"/Contents/MacOS/ARPGCompanion --socket <path> --tab tuning`.
This captures the companion's own view, without desktop capture or input injection.

A scenario's `tuning` timeline contains complete validated settings before the
named zero-based tick. See [runtime player tuning](../scenarios/runtime_player_tuning.ron)
and the monster, Cleave and Slam scenarios beside it. These verify timing and
physical consequences; whether a combination feels satisfying needs playtesting.
