---
name: playtest
description: Build, run and drive the arpg game to prove a change actually works — inject keys, screenshot the running app, and read simulation state as numbers over the ARPG_HARNESS control socket. Use whenever you need to see the game running, verify movement/camera/rendering behaviour, take a screenshot, or measure frame throughput. Do NOT reach for osascript keystrokes or the screencapture utility; they fail silently in this environment and have already cost hours.
---

# Playtesting arpg

The game can be driven entirely from a shell. **Do not automate it through the
OS.** Synthetic keystrokes need the window frontmost and accessibility
permission; desktop screenshots need the display awake and a crop guessed from
outside. None of that fails loudly — keys go to whatever *is* focused, images
come back black, and both look exactly like the game being broken. Everything
below works on an unfocused window buried behind others.

Rust is installed with `--no-modify-path`, so every command needs
`. "$HOME/.cargo/env" &&` first.

## Launching

```sh
. "$HOME/.cargo/env" && cargo build --release
rm -f /tmp/arpg.sock
ARPG_HARNESS=/tmp/arpg.sock RUST_LOG=arpg=info,arpg_gfx=info ./target/release/arpg
```

Run that last line **in the background**. Then wait for the socket to exist
before sending anything — the GPU takes a moment to come up:

```sh
until [ -S /tmp/arpg.sock ]; do :; done
```

Without `ARPG_HARNESS` there is no socket and no listener at all, so a plain
`cargo run` is unaffected by any of this.

Shut down with `echo quit | nc -U /tmp/arpg.sock` (exits 0) rather than `pkill`.

## Commands

Send one per connection. Every command replies, and **the reply means the effect
has landed** — so sequence commands instead of sleeping and hoping.

```sh
sock() { echo "$1" | nc -U /tmp/arpg.sock; }
```

| Command | Replies |
|---|---|
| `press <key>` / `release <key>` | immediately |
| `tap <key>` | immediately; key is held for exactly one frame |
| `hold <key> <ms>` | **after the key comes back up** |
| `wait <ms>` | after that much game time |
| `shot <path>` | **after the PNG is on disk** |
| `state` | one line of numbers (below) |
| `enemies <n>` | clamped count |
| `vsync on\|off` | resulting state |
| `quit` | then exits |

Keys come from `BINDINGS` in `crates/app/src/input.rs`, so whatever is bound is
drivable — currently `w a s d up down left right`. An unknown one replies
`error: unknown key "q"; bound keys are w s a d up down left right`, which is
also how to ask what exists. Malformed input is always reported, never ignored.

Meta commands say what they mean (`enemies 512`); do **not** simulate the debug
keys (`[`, `]`, `v`, `p`) to achieve the same thing.

## Reading state

```
player_pos 3.821 0.600 -3.821 facing 2.3562 camera_target 4.513 -4.513
enemies 1024 instances 17409 frames 4080 skipped 1 frame_ms 16.64 vsync true
```

## Verify by predicting first, then measuring

This is the point of the tool. Work out what the number should be from the
constants, *then* run it — a match is real evidence, and a mismatch tells you
which of the two is wrong.

Current constants (`crates/sim/src/lib.rs`, `crates/gfx/src/camera.rs`) —
**re-read them rather than trusting this list, they are tuning knobs**:

| | |
|---|---|
| `PLAYER_SPEED` | 9.0 world units/sec |
| `PLAYER_TURN_RATE` | 14.0 rad/sec |
| `ARENA_HALF` | 96.0 (128 tiles x 1.5) |
| camera `FOLLOW_HALF_LIFE` / `LEAD_HALF_LIFE` / `LOOK_AHEAD` | 0.12s / 0.35s / 4.0 |

Screen directions are world diagonals: screen-right is `(+X, -Z)/√2`, screen-up
is `(-X, -Z)/√2`. Yaw is `atan2(dir.x, dir.z)`, so due-east is `3π/4 ≈ 2.3562`.

```sh
sock "hold d 500" >/dev/null   # 500ms x 9 u/s / √2 = 3.182 per axis
sock state                      # -> player_pos 3.183 ... facing 2.3562
```

Expect agreement to within one frame of travel (~0.15 units at 60Hz), because
`dt` is still wall-clock. Exact reproducibility arrives with the fixed timestep.

## Screenshots

```sh
sock "shot /tmp/scratch/frame.png"   # replies once written
```

Then read the PNG directly — it is the exact surface, correctly framed, so no
cropping is needed. `P` in the app does the same thing interactively, writing to
`$ARPG_CAPTURE_DIR` (default: the temp dir).

## Measuring performance

Count frames over a known interval. Do **not** trust `frame_ms` alone — it is an
EMA and cannot tell a steady 60Hz from a mixture averaging to it.

```sh
a=$(sock state); sock "wait 1000" >/dev/null; b=$(sock state)
# presented = frames(b) - frames(a);  also diff `skipped`
```

Two ways this lies to you, and both have already happened:

1. **Skipped frames counted as speed.** An occluded window hands back no
   texture, the draw is skipped, and the loop spins freely — once reported
   *12,467 fps with vsync on*, entirely skipped frames. Diff `skipped` across
   the window; a small constant from startup is normal, a rising one is not.
2. **Background throttling, with `skipped` at zero.** A backgrounded app still
   presents, just slowly. Measured 63/s vsync and 65/s uncapped with zero
   skipped — versus 62/s and 302/s for the same build with the window up.

So the check that actually works: **measure both present modes and compare.** If
uncapped is not several times vsync, the app is throttled and the number is not
a measurement. Bring the window to the front and repeat.

Reference, window frontmost, 17409 instances on an M4: **62/s vsync at 16.59ms,
302/s uncapped at 3.62ms, 0 skipped.**

## After any change

```sh
. "$HOME/.cargo/env" && cargo clippy --workspace --all-targets -- -D warnings
. "$HOME/.cargo/env" && cargo test --workspace
```

Both must be clean. The `PostToolUse` hook in `.claude/settings.json` runs the
clippy line for you after Edit/Write **and after every Bash call**, blocking with
exit 2 and the errors on stderr — so a shell-based edit (a python or sed
heredoc) cannot slip past it. Costs ~0.1s when nothing changed, because cargo's
own staleness check does the work. You still have to run the tests.

## What this cannot tell you

- **Feel.** It proves the character moved 3.183 units where 3.182 was predicted.
  It cannot say whether the camera half-life or turn rate *feels* right. That is
  the owner's call and needs a human at the keyboard.
- **winit's event delivery.** Keys are injected at `KeyCode` into
  `Input::on_key`, so the real binding table and input state machine are
  exercised, but the OS→`window_event` path is skipped. Key-repeat filtering and
  focus-loss release are covered by unit tests only.
- **True input latency.** Injections always land at the same point in the frame,
  whereas a real keypress arrives anywhere inside the refresh interval. Any
  latency measured here is an idealised best case.

## Current gaps in the harness

- No mouse, and no scenario setup (no spawn/teleport). Testing combat will want
  the latter, built on `World::spawn` once entity storage exists.
- `state` is a hand-maintained format string; extend it as the sim grows.
