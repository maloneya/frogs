# Runtime tuning

The tuning panel is a separate native SwiftUI app. Build it once from the
repository root:

```sh
python3 scripts/tuning.py build
cargo run --release
```

Press **F4** in the game to open or focus **ARPG Tuning**. Alternatively,
`python3 scripts/tuning.py open` builds and opens it. Select a type in the sidebar,
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

## Connection and lifetime

The game starts a tuning-only Unix socket at `target/tuning.sock` relative to its
working directory. `ARPG_TUNING_SOCKET` overrides it; use a distinct path per game
instance. F4 passes that exact path to the companion. To open manually against
another socket, use `python3 scripts/tuning.py open --socket /tmp/experiment.sock`.
Close an existing companion before switching sockets: opening an already running
macOS app focuses its existing window.

The socket is user-readable/writable only. An active socket or a regular file is
never removed to start another server. A refused connection to a stale socket
allows replacement after a crash; clean shutdown removes the owned socket.

Each connection carries one newline-terminated JSON request and one JSON reply:

```json
{"op":"state"}
{"op":"set","session":12345,"key":"player_speed","value":6}
{"op":"save","session":12345}
{"op":"revert","session":12345}
{"op":"defaults","session":12345}
{"op":"restart","session":12345}
```

Use the `session` from the latest state reply for every mutation; the example
number above is a placeholder. A stale session is refused before any edit.
Replies contain `ok`, `error`, and a `state` snapshot with catalog metadata,
accepted and saved values, saved-file path, status, and game process identity.
Malformed requests return `ok: false` and an error. Input is bounded to 4096 bytes.
The socket thread handles I/O; the game's main loop validates and applies edits
between ticks, then replies. The request queue holds at most one command;
additional requests receive a busy error while the game is stalled. Expired
queued requests are rejected without mutation. A lost reply leaves the result uncertain: the companion refreshes
state and never automatically resends an edit.

The companion refreshes every half second, including changes made through the
harness. These small snapshots do not serialize the world. Drafts survive normal
refreshes. An edit reserves the controls while it waits for a refresh, so
clicks cannot queue out of order. Disconnects disable editing and discard drafts; reconnection fetches
a fresh catalog and accepted state. A changed process identity also clears drafts.
The game alone advances simulation; neither UI drawing nor socket polling steps it.
Performance trials refuse tuning mutations; close the companion when measuring
to avoid its polling/rendering overhead.

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
values; `state.tuning` reports persistence and error status. The harness has no
panel pointer, keyboard, capture, or window lifecycle commands.

`python3 scripts/tuning.py test` tests the native model and builds the app bundle.
Full repository verification includes that check. For a self-capture of the
native view, launch the built executable with
`ARPG_TUNING_CAPTURE=/tmp/tuning.png target/"ARPG Tuning.app"/Contents/MacOS/TuningCompanion --socket <path>`.
This captures the companion's own view, without desktop capture or input injection.

A scenario's `tuning` timeline contains complete validated settings before the
named zero-based tick. See [runtime player tuning](../scenarios/runtime_player_tuning.ron)
and the monster, Cleave and Slam scenarios beside it. These verify timing and
physical consequences; whether a combination feels satisfying needs playtesting.
