# Invariants in place today

Moved out of `CLAUDE.md`, which is read at the start of every session and had
grown a fifty-row inventory of enforcement that already exists in the code.

**This file is a snapshot, not the source of truth.** The mechanisms in the
right-hand column are. An entry here can silently disagree with the code, which
is precisely the failure `BINDINGS` was restructured to avoid — key names live
*in* the binding table so there is no second table to forget. The honest fix is
to mark each invariant at its definition site and generate this table; until
then, treat a disagreement as this file being wrong.

The ladder that governs where a new invariant goes lives in `CLAUDE.md`, because
that rule fires on every edit. The inventory below only matters when auditing.

| Invariant | Layer | Mechanism |
|---|---|---|
| `gfx` cannot name a simulation type | 0 | separate crates — `use arpg_sim::…` is E0432 |
| `gfx` cannot depend on `sim`; `sim` cannot depend on wgpu/winit | 1 | `crates/{gfx,sim}/build.rs`, run on every build |
| Enemy count stays within the instance budget | 0 | private field; `World::set_enemy_count` clamps |
| Zoom stays in a sane range | 0 | private field; `OrthoCamera::zoom_by` clamps |
| Aspect ratio survives a minimised window | 0 | `aspect_of` guards inside the camera |
| `Renderer.vsync` cannot desync from the surface | 0 | private field; `toggle_vsync` is the only writer |
| `Instance` padding is never written | 0 | private fields; `Instance::new` is the only door |
| No allocation or overflow at the extract seam | 0 | `InstanceSink` exposes `push` and nothing else |
| The buffer is reset once per frame | 0 | reset lives in `InstanceBuffer::sink()` |
| `Instance` is exactly 48 bytes | 1 | `const _: () = assert!(…)` beside the type |
| Rust vertex layout matches `shader.wgsl` | 2 | headless pipeline + draw test in `gfx/src/lib.rs` |
| Public API stays deliberate | 1 | `unreachable_pub = "deny"` |
| No dependency outside a crate's allowlist | 1 | `crates/{gfx,sim}/build.rs` — fails closed, so unknown crates are caught too |
| Tuning constants stay in their valid range | 1 | a `const _: () = assert!(…)` beside each one |
| The lead eases slower than the follow | 1 | const assert; swapping them reintroduces the whip |
| The player footprint is never square | 1 | const assert; a square one makes facing invisible |
| The yaw convention matches the shader | 3 | pixel-readback test in `gfx/src/lib.rs`, mutation-checked |
| Smoothing is frame-rate independent | 3 | `core::damp` and its tests, including the naive lerp failing the same check |
| The lint wall runs however the edit was made | 1 | `PostToolUse` hook matches Bash as well as Edit/Write |
| The sink's cap holds at its real value | 3 | unit tests in `core` |
| `sim` cannot name a key or a window | 1 | `crates/sim/build.rs`; `core` never names winit |
| A movement direction is unit-length or zero | 0 | private field; `MoveDir::new` is the only door, and it normalises |
| Movement never leaves the ground plane | 0 | `MoveDir::new` drops the Y component |
| Input edges are consumed exactly once | 0 | the clear lives in `InputState::sample`, the only reader |
| Two keys on one action cannot desync | 0 | `Input.down` tracks *keys*; the action set is derived, never stored |
| The binding table fits its bitset | 1 | `const _: () = assert!(BINDINGS.len() <= u32::BITS …)` |
| A frame's dt cannot teleport the player | 0 | `Accumulator::pending` caps ticks per frame; `Clock` no longer clamps, so the HUD sees the real hitch |
| The simulation cannot see a variable timestep | 0 | `Dt` is a unit struct with a private field — there is no room in the type for a wrong duration |
| A `Dt` can only come from the accumulator | 0 | private field in `sim::time`; even `sim`'s own tests go through `Accumulator::pending` |
| A stalled frame cannot spiral | 1 | `const _: () = assert!(MAX_TICKS_PER_FRAME >= 1)`, plus the discard beside it; unit test in `sim/time.rs` |
| Every field of `World` reaches its hash | 1 | `World::hash` destructures `Self` exhaustively — a new field is E0027 |
| A field that is bound but never hashed | 3 | `every_field_of_the_world_reaches_the_hash`; mutation-checked, and it caught a real gap |
| The simulation is reproducible tick for tick | 3 | `one_input_stream_replays_to_the_same_hash_every_tick` |
| Frame rate cannot change the simulation | 3 | `frame_rate_cannot_change_the_simulation` — 1, 2 and 4 ticks per frame compared by hash sequence |
| A steady-state frame allocates nothing | 3 | thread-local counting allocator; `a_steady_state_frame_allocates_nothing` |
| Interpolation cannot reach sim state | 0 | `World::extract` takes `&self`; there is no `&mut` to write a blended value back through |
| `Alpha` stays in `0..=1` | 0 | private field; `Accumulator::alpha` clamps and is the only mint besides the two endpoint consts |
| The drawn position is a blend, never the sim's own | 0 | `player_pos` and `player_pos_at` are separate methods answering separate questions |
| Every body remembers where it was, every tick | 3 | `the_previous_tick_is_the_previous_tick`, measured in the crowd where bodies actually move |
| The horde is interpolated, not just the player | 3 | `the_horde_is_interpolated_too`; three mutations escaped before it existed |
| A respawned horde does not streak in from the old one | 3 | `a_respawned_horde_is_drawn_standing_still` — uncapped, most frames run zero ticks, so this is visible |
| The drawn facing crosses ±PI the short way | 3 | `blend_angle` uses `shortest_arc`; `the_drawn_facing_crosses_the_pi_seam_the_short_way` |
| Speed and turn rate are the constants they claim | 3 | `the_rates_are_the_constants_they_say_they_are`, predicted from the constants rather than read back |
| Ground + horde + player fit one buffer | 3 | unit test in `sim` at the largest horde the clamp allows |
| The camera basis agrees with the projection | 3 | unit tests in `gfx/camera.rs` |
| Camera smoothing is frame-rate independent | 3 | `damp` uses `2^(-dt/half_life)`; unit test in `gfx/camera.rs` |
| The camera target cannot be set unsmoothed | 0 | private field; `follow` is the only writer |
| The camera never overshoots or bobs vertically | 3 | unit tests in `gfx/camera.rs` |
| Turning takes the short way round the ±PI seam | 3 | `shortest_arc` wraps the *difference*; unit test in `sim` |
| Turning is frame-rate independent and never overshoots | 3 | step clamped to the remaining arc; unit tests in `sim` |
| Facing cannot drift toward the precision limit | 3 | `wrap_angle` after every turn; unit test in `sim` |
| Spawning does not swoop the camera in from the origin | 0 | `snap_to`, called in `resumed` before the first frame |
| A skipped frame is never counted as a rendered one | 1 | `Renderer::render` is `#[must_use]`, so ignoring the result is a denied warning |
| Readback rows respect the copy alignment | 3 | `padded_bytes_per_row`; unit test in `gfx/capture.rs` |
| A malformed harness command is reported, not ignored | 3 | `parse` returns `Result`; unit test in `app/harness.rs` |
| A screenshot that never happened is not reported as `ok` | 3 | `capture_has_stalled`; unit tests in `app/app.rs` |
| A body's position cannot leave the ground plane | 0 | positions are `Vec2`; `on_ground` is the only lift |
| Nothing spawns already overlapping | 1 | `const _: () = assert!(ENEMY_SPACING > 2.0 * ENEMY_RADIUS)` |
| Coincident bodies separate deterministically, not into NaN | 3 | `escape_direction`; unit tests in `sim` |
| The harness cannot exist unless asked for | 0 | `harness::start` returns `None` without `ARPG_HARNESS` |
| The harness cannot fall behind the bindings | 0 | key names live *in* `BINDINGS`; there is no second table to forget |
