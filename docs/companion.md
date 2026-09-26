# ARPG Companion

Development controls live in a separate native macOS SwiftUI app. Build and run
from the repository root:

```sh
python3 scripts/dev.py
```

The launcher builds both applications, starts the game, waits for its tooling
socket to report the launched process identity, then opens a separate companion
instance against that socket. It exits after startup; closing either app leaves
the other running. A game restart lets the companion reconnect to the same socket.
The game keeps running while the companion has focus; focus loss releases input.
The game never launches the companion or knows its bundle path.

Use `python3 scripts/dev.py --no-build` to reuse built binaries. `--socket` selects
another endpoint; an active socket is refused without disturbing its owner.
Game output is written to `target/dev-game.log`. Startup failure reports that log
and stops only the game process started by that attempt.

To run either application independently:

```sh
cargo run --release
python3 scripts/companion.py open
```

The second command builds and opens only the companion. Packaged builds provide
`play.command` for the game and `tools.command` for both apps (Python 3 required).

- **Scenes:** select a file and click **Open selected**, or **New scene**, to edit
  its starting layout on a map aligned with the game camera. Place enemies,
  sources, formations, trees and campfires; drag walls into rectangles. Edit
  ambient and sun/moon illumination in Scene lighting. Select and drag objects to move them, or use the
  inspector for exact values. **Save & play** saves and starts the layout;
  **Restart scene** repeats the last loaded layout with current tuning.
  Refresh rescans `scenes/`. Startup reopens the last played file; on first launch, choose a scene in the companion.
  See [scene editing](scene-editor.md) and [scene lifecycle](scene-playtests.md).
- **Tuning:** edit type-wide values at runtime, save them, revert saved values,
  or reset defaults. See [runtime tuning](tuning.md) for application timing and
  persistence.
- **Debug:** toggle collision outlines and vsync, adjust camera zoom, read frame
  timing, enemy and render instance counts, and simulation tick, and save a game screenshot through a
  native save dialog. Screenshot status changes from pending to saved only after
  the renderer writes the PNG; overlap, write failures, restart cancellation and
  the four-second capture deadline are reported. The frame readout is smoothed;
  use the [performance harness](performance.md) for measurements.

The in-game debug menus and their F2/F3/V/P/zoom shortcuts are removed. The game
retains movement, attacks, interaction, and Escape to
quit. Health, ability status, encounter progress, and combat feedback remain in
the gameplay HUD. Collision geometry is still drawn in the game when enabled.

## Connection and lifetime

The game starts a companion Unix socket at `target/companion.sock` relative to its
working directory. `ARPG_COMPANION_SOCKET` overrides it; use a distinct path per game
instance. The development launcher supplies that exact path to both apps. To open manually against
another socket, use `python3 scripts/companion.py open --socket /tmp/experiment.sock`.
The launcher and companion command open a new native instance so the requested
socket is honored even when a companion for another game is already running.

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
{"op":"refresh_scenes","session":12345}
{"op":"start_scene","session":12345,"path":"scenes/soul_trial.ron"}
{"op":"collision","session":12345,"enabled":true}
{"op":"vsync","session":12345,"enabled":false}
{"op":"zoom","session":12345,"direction":"in"}
{"op":"capture","session":12345,"path":"/tmp/arpg.png"}
```

Use the `session` from the latest state reply for every mutation; the example
number above is a placeholder. A stale session is refused before any command.
Replies contain `ok`, `error`, and a `state` snapshot with catalog metadata,
accepted and saved values, saved-file path, status, game process identity, scene catalog, and presentation readouts.
Malformed requests return `ok: false` and an error. Input and native responses are bounded to 8 MiB; ordinary state replies remain small.
The socket thread handles I/O; the game's main loop validates and applies edits
between ticks, then replies. The request queue holds at most one command;
additional requests receive a busy error while the game is stalled. Expired
queued requests are rejected without mutation. A lost reply leaves the result uncertain: the companion refreshes
state and never automatically resends an edit.

The companion refreshes every half second, including changes made through the
harness. These small snapshots do not serialize the world. Drafts survive normal
refreshes. An edit reserves the controls while it waits for a refresh, so
clicks cannot queue out of order. Disconnects disable submitting edits and discard unsubmitted tuning text; reconnection fetches
a fresh catalog and accepted state. A changed process identity also clears tuning text drafts.
Scene documents are returned only by `scene_editor` requests, never by polling.
Their accepted drafts and undo history survive disconnection and process changes;
only submitting further edits requires a connection. See [editor protocol](scene-editor.md#protocol).
The game alone advances simulation; neither UI drawing nor socket polling steps it.
Performance trials refuse companion commands other than state queries; close the companion when measuring
to avoid its polling/rendering overhead.

## Verification

`python3 scripts/companion.py test` checks the native model and builds the bundle;
full repository verification includes it. App tests cover scene start/restart,
invalid files and retry, stale sessions, trial refusal, zoom, collision outlines,
and capture completion. The opt-in [harness](../.claude/skills/playtest/SKILL.md)
remains available for automation; it has no simulated in-game menus.

For a view capture without desktop capture or synthetic input:

```sh
ARPG_COMPANION_CAPTURE=/tmp/scenes.png target/"ARPG Companion.app"/Contents/MacOS/ARPGCompanion --socket /tmp/experiment.sock --tab scenes
```

`--tab` selects `scenes`, `tuning`, or `debug` at launch. The screenshot contains
the companion's own view. Use its Debug tab or harness `shot` for a game frame.
`ARPG_COMPANION_TEST_SOCKET` enables live client checks against an isolated game;
use a temporary `ARPG_TUNING_FILE` because those checks exercise saving.
