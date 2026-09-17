---
name: playtest
description: Drive the running arpg app through ARPG_HARNESS for visual checks, input integration, screenshots and render measurements. Use the engine socket rather than OS key injection or desktop capture; use scenarios for headless behavioural assertions.
---

# Live playtesting

Use the game's harness for input, state, trace and capture. OS key injection can
reach the wrong app; desktop capture depends on focus and permissions. Harness
input works without focus, but screenshots still require presenting frames:
keep the window visible and the display awake. Search [traps](../../../docs/traps.md)
when observations are surprising.

## Launch and drive

```sh
. "$HOME/.cargo/env"
cargo build --release
ARPG_HARNESS=/tmp/arpg.sock RUST_LOG=arpg=info,arpg_gfx=info ./target/release/arpg
```

Launch the last command as a background process using your execution tool.
Wait for the socket with a bounded timeout and check startup output if it fails.
Use a fresh socket path if another instance is running; do not remove its socket.
Send one command per connection:

```sh
sock() { echo "$1" | nc -U /tmp/arpg.sock; }
sock state
sock 'hold d 500'
sock 'shot /tmp/arpg-frame.png'
sock quit
```

Each command replies at its documented boundary. `hold` waits for release;
`shot` waits for the PNG write or an error; `spawn` replies when queued and the
body lands on a later tick. The game continues stepping between commands.

| Operation | Commands |
|---|---|
| Input | `press`, `release`, `tap`, `hold <key> <ms>` |
| Observe | `state`, `trace since <tick>`, `wait <ms>`, `shot <path>` |
| Bulk population | `enemies <n>`, then `seekers <n>` if desired |
| Place or push | `spawn <x> <z> [seek]`, `impulse <player\|#id> <x> <z>` |
| Sources | `source <x> <z> [flags]`, `source remove\|enable\|disable <id>` |
| Scenes | `scene start <path>`, `scene restart`, `scene add <path>`, `scene evict <id>`, `scene list` |
| Assets | `asset\|character\|horde show <path.glb>`, `asset\|character\|horde clear` |
| Presentation | `vsync on\|off`, `debug collision on\|off` |
| Exit | `quit` |

The command parser is in [harness](../../../crates/app/src/harness.rs). Source
flags include cadence, placement, conditions and initial enablement; see
[source enablement](../../../docs/source-enablement.md). `template` consumes the
remaining text as the owning RON type, so place other flags before it.

Key names come from [BINDINGS](../../../crates/app/src/input.rs); an unknown key
reply lists them. `tap space` swings and `tap e` interacts. F1 opens the attack
picker; F2 opens the scene picker. W/S or arrows navigate, Enter confirms,
Escape cancels. While a menu is open it captures gameplay input; the world keeps
running. Use explicit harness commands for debug operations instead of trying
to inject native debug keys.

`enemies` resets enemy identities and behaviours; props survive. Apply `seekers`
afterwards to make the new horde chase. Scene restart restores the cached
snapshot; starting the file again rereads it. Discard old IDs after a new run.
See [scene playtests](../../../docs/scene-playtests.md).

## Observe and predict

Read JSON by field name with `jq`:

```sh
sock state | jq '.sim.tick, .sim.player_pos'
sock state | jq '.render.frames, .render.skipped'
sock state | jq '.sim.bodies'
```

Body reports are keyed by stable ID and include position, carried velocity and
inverse mass. State is derived from authoritative data, but reporting changes
still require verification; adding a storage field does not automatically make
its value visible. Trace shows transitions between state samples. A dropped
event count means history was truncated.

Predict from the current owning constants before measuring. A walk in open
ground covers speed times fixed tick duration per tick. A wall-clock `hold`
does not guarantee an exact tick count, and contact or carried motion can alter
the result. Use a scenario when exact tick timing is the question. Explain the
expected effect and observed result separately from whether it feels good.

## Screenshots and performance

`shot` writes the app's own image; inspect that file directly. A covered window
can time out. Capture also reports write failure and rejects a second request
while one is pending. F3 or `debug collision on` displays authoritative collision
geometry; see [collision debug](../../../docs/collision-debug.md).

Measure release builds with the window visible and frontmost and display awake.
Record the scene, population and present mode. Compare rendered frames, skipped
frames and simulation ticks across the same timed interval. Repeat short samples
and report their spread; the best sample estimates attainable throughput under
intermittent background throttling, not typical frame pacing.

Measure uncapped to expose throughput differences and under vsync for play feel.
Similar rates can mean throttling, a real bottleneck or a workload already near
the display limit; investigate rather than declaring the measurement invalid.
The frame-time EMA hides variance. For simulation cost alone, use the scenario
runner's step budget. Keep historical numbers out of current tuning decisions.

For presentation smoothing, use `arpg_core::damp` or `damp_vec3`: their elapsed-time
half-life stays consistent across frame rates. A fixed fraction per frame does
not. Compare at different frame rates when changing camera or animation timing.

## Finish

Run the [repository checks](../../../CLAUDE.md#checks-and-commands), including
scenario assertions for changed simulation/gameplay behaviour. A screenshot or
manual reading of state cannot replace those assertions. Report how the user
can repeat the trial, what was verified, and the remaining feel question.

The harness exercises bindings and app input handling but bypasses OS event
delivery. Its injection timing does not measure real keyboard latency. It also
cannot judge whether the combat is satisfying; that needs the user's playtest.
