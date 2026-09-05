---
name: playtest
description: Build, run and drive the arpg game to prove a change actually works — inject keys, screenshot the running app, read simulation state as JSON, and read the tick-stamped event trace, all over the ARPG_HARNESS control socket. Use whenever you need to see the game running, verify movement/camera/rendering behaviour, take a screenshot, measure frame throughput, or ask what the simulation did between two moments rather than what it holds now. Do NOT reach for osascript keystrokes or the screencapture utility; they fail silently in this environment and have already cost hours.
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
| `shot <path>` | **after the PNG is on disk**, or an error if no frame presented |
| `state` | one line of numbers (below) |
| `trace since <tick>` | every event from that tick on, plus a `# n event(s)` count |
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

```json
{"sim":{"tick":1836,"player_pos":[3.8210,0.6000,-3.8210],"facing":2.3562,
 "contacts":0,"enemies":1024,"trace_events":57,"trace_dropped":0,
 "player_prev_pos":[3.6710,0.6000,-3.6710],"prev_facing":2.3562},
 "render":{"camera_target":[4.5130,0.0000,-4.5130],"instances":17409,
 "frames":4080,"skipped":1,"frame_ms":16.6400,"vsync":true}}
```

**Read it with `jq`, never by column.** The `sim` half is *derived* — `World`
is destructured exhaustively, so a field added to the world will not compile
until it is reported — which means fields appear here without warning and any
positional reader breaks. That has already happened once, when `tick` was added
to the front of the old space-separated line.

```sh
sock state | jq -r .sim.tick
sock state | jq -r '.sim.player_pos[0]'
sock state | jq -r '.render.frames, .render.skipped'
```

## Reading the trace

`state` is a point sample; `trace` is the interval between two of them. Anything
with a window — an attack, hitstop, a buffered input — exists only in that gap.

```sh
a=$(sock state | jq .sim.tick)   # the tick right now
sock "hold d 400" >/dev/null
sock "trace since $a"
# 328 contacts count=4
# 329 contacts count=1
# ...
# # 25 event(s)
```

Same format as a scenario's golden trace file, because it is the same function.
A `# n event(s) dropped` line means the ring buffer wrapped and the beginning is
gone. An empty result is `# 0 event(s)`, not an error.

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
sock state | jq -r '.sim.player_pos[0]'   # -> 3.1830
```

Travel is now an exact multiple of one tick's step — `PLAYER_SPEED * Dt::SECS`
= 0.15 units — because the simulation only advances in whole ticks. Measured:
`hold d 500` in open ground moved exactly 4.5000 units, 3.182 per axis.

So predict in **ticks**, not milliseconds. The remaining slack is entirely in
*how many ticks the key was actually down*: a `hold` is wall clock, and the
socket round-trips either side of it are worth a tick or two. Above, 32 ticks
elapsed and exactly 30 of them had the key held. If a distance is not a multiple
of 0.15, something is touching the player — check `contacts`.

## Screenshots

```sh
sock "shot /tmp/scratch/frame.png"   # replies once the file is on disk
```

Read the PNG directly — it is the exact surface, correctly framed, so no
cropping is needed. `P` in the app does the same interactively, writing to
`$ARPG_CAPTURE_DIR` (default: the temp dir).

**A fully occluded window is the one thing here that breaks.** Unfocused is
fine; covered is not. It now fails loudly rather than replying `ok` with no file
on disk — see `docs/traps.md`, first entry.

## Measuring performance

Count frames over a known interval. Do **not** trust `frame_ms` alone — it is an
EMA and cannot tell a steady 60Hz from a mixture averaging to it.

```sh
a=$(sock state | jq .render.frames); sock "wait 1000" >/dev/null
b=$(sock state | jq .render.frames); echo $((b - a))
```

Diff `.render.skipped` too, and **measure both present modes**: if uncapped is
not several times vsync, the app is throttled and the number is not a
measurement. Both ways this lies are in `docs/traps.md`, keyed by what you see.

Also diff `.sim.tick`. It should hold ~60/s in *both* modes — that is the fixed
timestep working, and a tick rate that follows the frame rate is a real bug.

Reference, window frontmost, 17409 instances on an M4: **62/s vsync,
~300/s uncapped, 0 skipped.**

## This is not the completion gate

The harness proves a change *ran*, not that it is *correct* — the comparison
happens in your head. That is rule 4 in `CLAUDE.md`; write the prediction as a
scenario assertion instead, which costs the same and persists.

Use this skill for what a scenario cannot do: looking at the running game,
checking an image, and measuring render cost.

## Before diagnosing anything surprising

Grep `docs/traps.md`. Occluded-window screenshots, skipped frames read as speed,
and OS keystroke injection are all already in there, keyed by what you see.

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

## Before changing a tuning constant

Feel constants (`PLAYER_SPEED`, `PLAYER_TURN_RATE`, `FOLLOW_HALF_LIFE`,
`LEAD_HALF_LIFE`, `LOOK_AHEAD`, zoom, `MAX_FRAME_TIME`) each carry a
`const _: () = assert!(…)` guarding their valid range, so the catastrophic edits
fail to compile. That is a floor, not a check on the value being *right*: state
the effect you expect, change it, then measure it with the recipe above and put
the number in the commit message.

For anything needing smoothing, use `arpg_core::damp` / `damp_vec3`. Do not
write `lerp(a, b, 0.1)` once a frame — it keeps 90% of the error per *frame*
rather than per second, so it converges thousands of times faster at 144Hz than
at 60Hz and the game feels different on faster hardware.

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
- `state`'s `sim` half is derived and cannot silently omit a field; the
  `render` half is still hand-written, because `app` has no single struct to
  destructure. `tick` is simulation time and `frames` is wall clock — they are deliberately
  different numbers now, and their ratio is what says whether the machine is
  keeping up. Uncapped, `frames` runs far ahead of `tick`; that is the fixed
  timestep working, not a fault.
