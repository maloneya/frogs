---
name: playtest
description: Drive arpg through ARPG_HARNESS for visual checks, input integration, screenshots and render measurements. Use scenarios for headless behavioural assertions.
---

# Live playtesting

Use the game harness, not OS key injection or desktop capture. Harness input
works without focus; screenshots require presenting frames, so keep the window
visible and display awake. Search [traps](../../../docs/traps.md) for surprising
observations. For human authoring, use the [scene editor](../../../docs/scene-editor.md).

## Launch and drive

```sh
. "$HOME/.cargo/env"
cargo build --release
ARPG_HARNESS=/tmp/arpg.sock ARPG_COMPANION_SOCKET=/tmp/arpg-tools.sock ARPG_TUNING_FILE=/tmp/arpg-tuning.ron RUST_LOG=arpg=info,arpg_gfx=info ./target/release/arpg
```

Run the last command in the background with your execution tool. Choose unused
paths for each session; never remove another instance's socket. Wait with a bounded
timeout and inspect startup output on failure. Harness sessions start empty and
skip the interactive scene preference, so load content explicitly:

```sh
sock() { echo "$1" | nc -U /tmp/arpg.sock; }
sock 'scene start scenes/seekers.ron'
sock state
sock 'hold d 500'
sock 'shot /tmp/arpg-frame.png'
sock quit
```

Send one command per connection. `hold` replies after release; `shot` after PNG
write/error; `spawn` when queued, before its body lands on the next tick. Game
keeps stepping between commands.

| Operation | Commands |
|---|---|
| Input | `press`, `release`, `tap`, `hold <key> <ms>`, `cursor <x> <y>` |
| Observe | `state`, `trace since <tick>`, `wait <ms>`, `shot <path>` |
| Performance | `perf`, `perf samples`, `perf reset` |
| Place or push | `spawn (pos: (<x>, <z>), what: (physical: (health: Some(3), character: true), role: Hostile, actions: [Move((destination: Target(Player), speed: 2.5, acceleration: 30.0, stop_distance: 0.55))], behavior: Action(0)))`, `impulse <player\|#id> <x> <z>` |
| Sources | `source (pos: (<x>, <z>), every: <ticks>)`, `source remove\|enable\|disable <id>` |
| Scenes | `scene start <path>`, `scene restart`, `scene add <path>`, `scene evict <id>`, `scene list` |
| Assets | `character\|horde show <path.glb>`, `character\|horde clear` |
| Tuning | `tuning set <key> <value>`, `tuning save\|revert\|defaults\|restart` |
| Presentation | `vsync on\|off`, `debug collision on\|off` |
| Exit | `quit` |

[Harness](../../../crates/app/src/harness.rs) owns command syntax; game owns complete RON
actor/source payloads. Key names come from [BINDINGS](../../../crates/app/src/input.rs);
unknown-key replies list them. `cursor` uses physical window pixels and the native
ground-projection path. See [controls](../../../README.md#controls) and [aiming](../../../docs/aiming.md).

Use authored scenes for repeatable setups, one-off spawn/impulse commands for
probing. [Scene lifetime](../../../docs/scene-playtests.md) defines start versus
restart and run-scoped IDs. [Companion](../../../docs/companion.md) documents its
separate socket, launching and native self-capture.

## Predict and observe

Read JSON by field name:

```sh
sock state | jq '.sim.tick, .sim.player_pos, .sim.bodies'
sock state | jq '.render.frames, .render.skipped'
```

Bodies are keyed by stable ID, with position, physical velocity and inverse mass;
`actors` reports chosen action, committed target, progress and cooldowns. Trace records transitions; dropped
events mean history was truncated. Verify that new reported fields reflect the
owner rather than assuming storage automatically makes them visible.

Predict from current owning constants. Movement accelerates toward its requested
speed through the shared bounded motor; contact forces and carried momentum
affect displacement. Wall-clock `hold` does not guarantee a tick count. Use [scenarios](../scenario/SKILL.md)
for exact timing and assertions. Keep measured behaviour separate from feel.

## Capture and performance

Inspect the file written by `shot`. Covered windows can time out; overlapping
captures and write failures are reported. `debug collision on` exposes
[authoritative outlines](../../../docs/collision-debug.md).

Measure release builds with a visible frontmost window and awake display. Close
the companion, warm up the chosen workload, reset telemetry, measure, then export
once. Record population, present mode, rendered/skipped frames and ticks. Repeat
samples and report variation. Uncapped runs measure throughput; vsync runs help
assess pacing. Smoothed FPS and renderer CPU time do not establish GPU duration.
The [performance guide](../../../docs/performance.md) owns timing boundaries and
interpretation; use its benchmark runner for regression comparisons:

```sh
python3 scripts/perf.py run --mode live --output target/perf/new-run.json
```

Exit 2 is inconclusive, never a pass. Preserve gates and baseline evidence.
For camera/animation changes use core's elapsed-time half-life helpers and
compare frame rates; fixed per-frame fractions change behaviour with FPS.

Run the [repository checks](../../../docs/verification.md), then report the
repeatable trial, evidence and remaining feel question. Harness injection tests
bindings/app handling, not OS event delivery or physical keyboard latency.
