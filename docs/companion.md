# ARPG Companion

The native macOS app hosts [scene authoring](scene-editor.md),
[runtime tuning](tuning.md) and diagnostics. The game owns simulation and exposes
a tooling socket; it never launches the companion or knows its bundle path.
Scene authoring uses the bundled [headless backend](scene-authoring.md) and works
without the game. Only playing scenes, tuning and diagnostics use its socket.

## Launch

From the repository root:

```sh
python3 scripts/dev.py
```

The launcher builds both apps, starts the game, checks that the ready socket
belongs to that process, opens the companion, then exits. Either app can close
independently; a relaunched game lets the companion reconnect on the same socket.
Focus loss releases game input while simulation continues.

Use `--no-build` to reuse binaries or `--socket /tmp/experiment.sock` for another
instance. Active sockets are refused without disturbing their owner. Startup
failure stops only the newly launched game; output is in `target/dev-game.log`.
To run either app alone:

```sh
cargo run --release
python3 scripts/companion.py open
```

The companion command builds the native app and bundles `arpg-author`, accepts
`--socket`, and opens a new native instance with an absolute scene-directory path.
The native executable accepts `--scenes <directory>`; without it, the catalog
uses `scenes/` relative to its working directory. File browsing is local to the
companion and works offline.
Packaged builds provide `play.command` for the game and `tools.command` for both
(Python 3 required).

## Tabs

- **Scenes:** compose starting layouts; see the [editor guide](scene-editor.md).
- **Tuning:** change type-wide movement and combat values; see [timing and persistence](tuning.md).
- **Debug:** collision outlines, vsync, camera zoom, frame/population readouts and
  game screenshots. Capture reports success only after the PNG write; overlapping
  requests, write failures, restart cancellation and the four-second deadline
  are reported. The frame display is smoothed; use [performance telemetry](performance.md)
  for measurements.

## Connection and protocol

The game socket defaults to `target/companion.sock` in its working directory;
`ARPG_COMPANION_SOCKET` overrides it. The launcher supplies the same absolute path
to both apps. The socket is user-only. Active sockets and regular files are never
removed; a refused connection permits stale-socket recovery. Clean shutdown removes
the owned socket.

Each connection carries one newline-terminated JSON request/reply. Start with
`{"op":"state"}`. Mutations include the returned `session` identity:

```json
{"op":"set","session":12345,"key":"player_speed","value":6}
{"op":"restart","session":12345}
{"op":"capture","session":12345,"path":"/tmp/arpg.png"}
```

The game vocabulary is [Control](../crates/app/src/companion.rs). Authoring
documents never travel through this socket. Replies contain
`ok`, `error` and a small `state` snapshot with accepted/saved tuning, catalog,
status and presentation readouts. They never serialize runtime bodies. Messages
are bounded to 8 MiB; stale sessions and malformed input are refused.

A socket thread handles I/O; the main loop validates/applies commands between
ticks. The queue admits one command, rejects excess requests as busy, and expires
unserved requests without mutation. Lost replies leave the result uncertain:
refresh state, never automatically resend an edit.

The companion polls every half second. Edits reserve controls before waiting
for an outstanding poll, preventing reordered clicks. Disconnect/process changes
clear unsubmitted tuning text; accepted scene drafts and undo history survive.
Reconnect before submitting runtime controls. Authoring continues offline.
Neither polling nor UI drawing steps Game.
Performance trials permit only state queries; close the companion for measurements
to avoid its polling/rendering overhead.

## Checks and capture

`python3 scripts/companion.py test` checks the native model and builds the bundle;
[full verification](verification.md) includes it. For native view capture:

```sh
ARPG_COMPANION_CAPTURE=/tmp/scenes.png target/"ARPG Companion.app"/Contents/MacOS/ARPGCompanion --socket /tmp/experiment.sock --tab scenes --scene /absolute/path/scene.ron
```

`--tab` accepts `scenes`, `tuning` or `debug`; optional `--scene` opens a draft
without waiting for the game connection. The bundled backend is used by default;
`ARPG_AUTHORING_BIN` overrides its executable for development/tests.
Capture contains only the companion's view. Use Debug or
harness `shot` for a game frame. `ARPG_COMPANION_TEST_SOCKET` enables live client
checks against an isolated game; give that game a temporary `ARPG_TUNING_FILE`
because the checks save settings. See the [playtest guide](../.claude/skills/playtest/SKILL.md)
for automation.
