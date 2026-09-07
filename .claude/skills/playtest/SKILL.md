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
| `enemies <n>` | clamped count, **and the seeker count**, which a respawn resets to 0 |
| `impulse <player\|#id> <x> <z>` | momentum applied; movement starts on the next tick |
| `seekers <n>` | how many of the bodies now chase the player |
| `spawn <x> <z> [seek]` | `queued`, not `spawned` — it lands on the next tick |
| `source <x> <z> [flags]` | the source's name, e.g. `source s0` |
| `source remove <id>` | takes the name as printed, `s0` |
| `vsync on\|off` | resulting state |
| `quit` | then exits |

Keys come from `BINDINGS` in `crates/app/src/input.rs`, so whatever is bound is
drivable — currently `w a s d up down left right space f1 escape r`.
`tap f1` opens the attack tuning panel, `tap left` / `tap right` edits recovery,
`tap r` resets it, and `tap escape` closes it. These go through the same modal
input route as native keys. While open, the panel captures gameplay input but
the world keeps ticking. An edit is accepted immediately and applies before the
next sim tick; `state.ui.recovery_pending` distinguishes those moments.
`state.sim.recovery_ticks` is the next swing's applied setting;
`state.sim.swing_recovery_ticks` is the in-flight setting, or zero when idle.
Settings last for this run only. `space` swings, and it
is an edge, so `tap space` is the right way to ask for exactly one. An unknown one replies
`error: unknown key "q"; bound keys are w s a d up down left right`, which is
also how to ask what exists. Malformed input is always reported, never ignored.

Meta commands say what they mean (`enemies 512`); do **not** simulate the debug
keys (`[`, `]`, `v`, `p`) to achieve the same thing.

`seekers` is how to see the *bulk* horde move at all: bodies from `enemies <n>`
spawn inert, and chasing is a behaviour granted to them. Order matters, because
`enemies <n>` respawns the horde and retires every name — which revokes every
behaviour with them. So it is `enemies 200` *then* `seekers 200`, and the reply
to the first says `seekers 0` to make that hard to miss.

`spawn` and `source` are the other way in, and they go through the simulation's
own doors rather than a debug dial. `spawn` queues one body, which appears on
the next tick — the reply says `queued` for exactly that reason, so a `state`
taken immediately after does not look like a bug. `source` adds something that
keeps asking:

```sh
sock "source 12 0 seek every 20 ring 4 near 14"   # a nest, only while the player is close
sock "source 40 0 fewer 8"                        # keeps eight bodies alive
sock "source remove s0"                           # and the flow stops
```

Flags are named, in any order: `seek`, `every <ticks>`, `ring <radius>`,
`near <radius>` (fire only while the player is within it), `fewer <n>` (fire
only while the horde is smaller than that). No flags means every tick, forever,
on one spot. `trace since` then says which source asked for what:

```
422 fired source=s0
422 placed id=#2v2
```

**The game keeps running between your commands.** A source at `every 20` makes
three bodies a second while you are thinking, so count over a `wait` you asked
for rather than across two tool calls.

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

**Take the best of several short samples, never one long one.** macOS throttles
a window that is not frontmost, and it does so *intermittently* — twelve
consecutive one-second samples of an unchanged scene have measured anywhere from
63 to 331 frames/s, with `skipped` at zero throughout. A single sample is
therefore not a measurement, and averaging is worse than useless: the noise is
one-sided, because throttling only ever removes frames. The maximum is the only
estimator a throttled second cannot drag down.

```sh
best=0
for i in $(seq 1 8); do
  a=$(sock state | jq .render.frames); sock "wait 500" >/dev/null
  b=$(sock state | jq .render.frames)
  f=$(( (b - a) * 2 )); [ $f -gt $best ] && best=$f
done; echo $best
```

The tell that you are looking at throttling rather than cost is a result that is
not merely noisy but *backwards* — a heavier scene measuring faster than a
lighter one. Check that before believing any conclusion about cost.

Diff `.render.skipped` too, and **measure both present modes**: if uncapped is
not several times vsync, the app is throttled and the number is not a
measurement. Both ways this lies are in `docs/traps.md`, keyed by what you see.

Also diff `.sim.tick`. It should hold ~60/s in *both* modes — that is the fixed
timestep working, and a tick rate that follows the frame rate is a real bug.

Reference, window frontmost, 17409 instances on an M4: **66/s vsync,
~396/s uncapped, 0 skipped** — best of eight half-second samples. The horde is
nearly free to draw: 16385 instances (no enemies) measures within 2% of 17409,
which is instancing working, and the 16384 ground tiles dominating either way.

Both failure modes are in `docs/traps.md`, keyed by what you see. The short
version: `frames/s=0` with ticks still at 60 means the display is asleep or the
window is covered, and wild sample-to-sample variance means it is not
frontmost.

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
  `Controls::on_key`, so the real binding table and input state machine are
  exercised, but the OS→`window_event` path is skipped. Key-repeat filtering and
  focus-loss release are covered by unit tests only.
- **True input latency.** Injections always land at the same point in the frame,
  whereas a real keypress arrives anywhere inside the refresh interval. Any
  latency measured here is an idealised best case.

## Current gaps in the harness

- No mouse, and no way to teleport the player — it can only be walked. Placing
  *bodies* is covered by `spawn` and `source`.
- `source` cannot describe every source the simulation can hold: the scenario
  language is the full one. Anything the flags cannot say belongs in a `.ron`
  file, which is where it should be asserted anyway.
- `state`'s `sim` half is derived and cannot silently omit a field; the
  `render` half is still hand-written, because `app` has no single struct to
  destructure. `tick` is simulation time and `frames` is wall clock — they are deliberately
  different numbers now, and their ratio is what says whether the machine is
  keeping up. Uncapped, `frames` runs far ahead of `tick`; that is the fixed
  timestep working, not a fault.


Physical state lives under `sim.bodies`, keyed by stable entity name. Each entry
reports ground-plane `pos`, carried `velocity`, and `inverse_mass`. Use these
names as impulse targets, or `player` for the persistent player body. `impulse`
uses sim's validated input type; it cannot silently accept NaN or infinity.
