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
| Enemy count stays within the instance budget | 0 | `enemies` is private; `set_enemy_count` clamps, `place` refuses at `MAX_ENEMIES` |
| Zoom stays in a sane range | 0 | private field; `OrthoCamera::zoom_by` clamps |
| Aspect ratio survives a minimised window | 0 | `aspect_of` guards inside the camera |
| `Renderer.vsync` cannot desync from the surface | 0 | private field; `toggle_vsync` is the only writer |
| `Instance` padding is never written | 0 | private fields; `Instance::new` is the only door |
| No allocation or overflow at the overlay seam | 0 | `QuadSink` exposes `push` and nothing else |
| A `Quad`'s UVs and colour cannot be set by hand | 0 | private fields; `Quad::solid` / `Quad::textured` are the doors |
| `Quad` is exactly 48 bytes | 1 | `const _: () = assert!(…)` beside the type |
| The atlas row satisfies the copy alignment | 1 | `const _: () = assert!(ATLAS.is_multiple_of(256), …)` in `gfx/text.rs` |
| The white texel's UV and the texel itself cannot disagree | 1 | `WHITE_UV` is const-derived from `WHITE` in `gfx/text.rs` — one definition, not two |
| No glyph is packed over the white texel | 3 | `assert!` at the end of `rasterise`, so it fires in the real binary rather than only under test |
| Only `gfx` can mint a textured quad | 0 | `Quad::textured` is `pub(crate)`; the atlas and the type are in one crate |
| The overlay projects pixels the right way up | 3 | pixel test in `gfx/src/lib.rs`, mutation-checked against a flipped y |
| A readout's text stays inside its panel | 3 | `hud` layout tests, which need no GPU |
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
| Two keys on one action cannot desync | 0 | `Controls.accepted` tracks *keys*; the action set is derived, never independently maintained |
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
| A pass cannot stamp an event with the wrong tick | 0 | `TraceSink` is bound to the tick at creation and exposes only `emit` |
| A pass touches only what it declares | 0 | passes take declared slices and restricted sinks, not `&mut World`; the player shares body storage |
| The trace cannot be written by anything but a pass | 0 | `Trace::sink` is `pub(crate)`; `World::trace()` hands out `&Trace` |
| Perception cannot change what it observes | 0 | `World::trace()` returns `&Trace`, as `extract` takes `&self` |
| A world field cannot be unobservable | 1 | `World::report` destructures `Self`; a new field is E0027 until it is reported |
| A redraw cannot consume a keypress | 0 | presentation reads `InputState::held`, which cannot clear; only `sample` clears, and only a tick calls it |
| The docs cannot cite code that no longer exists | 3 | `every_identifier_the_docs_cite_exists_in_the_source`; found three real drifts on its first run |
| The trace is not part of sim state | 1 | `World::hash` destructures it and discards it explicitly; a new field would be E0027 |
| A truncated golden trace is never blessed | 3 | the runner refuses to compare or write when `Trace::dropped() > 0` |
| A golden trace is not coupled to world construction | 3 | the runner clears the trace after setup, so it records the run |
| A scenario cannot need a GPU or a window | 1 | `crates/scenario/build.rs` allowlist — `arpg-gfx` and `winit` are absent and fail closed |
| Sim behaviour changes are gated on an exit code | 1 | `Stop` hook runs `scenario -- scenarios/`; both halves live since chunk 3 |
| Every scenario is also a determinism test | 0 | `check_replay` runs unconditionally in the runner; a scenario cannot opt out |
| A scenario asserting a field the runner ignores is rejected | 1 | `#[serde(deny_unknown_fields)]` — a silently-ignored assertion is worse than a refused one |
| A checkpoint cannot change the simulation it observes | 0 | `check_state` takes `&World`, shared with final-state checks |
| A checkpoint cannot silently go unevaluated | 3 | runner validation rejects ticks outside the budget and checkpoint traces; CLI tests cover invalid definitions and transient failures, including under `--bless` |
| Interpolation cannot reach sim state | 0 | `World::extract` takes `&self`; there is no `&mut` to write a blended value back through |
| `Alpha` stays in `0..=1` | 0 | private field; `Accumulator::alpha` clamps and is the only mint besides the two endpoint consts |
| The drawn position is a blend, never the sim's own | 0 | `player_pos` and `player_pos_at` are separate methods answering separate questions |
| Every body remembers where it was, every tick | 3 | `the_previous_tick_is_the_previous_tick`, measured in the crowd where bodies actually move |
| The horde is interpolated, not just the player | 3 | `the_horde_is_interpolated_too`; three mutations escaped before it existed |
| A respawned horde does not streak in from the old one | 3 | `a_respawned_horde_is_drawn_standing_still` — uncapped, most frames run zero ticks, so this is visible |
| The drawn facing crosses ±PI the short way | 3 | `blend_angle` uses `shortest_arc`; `the_drawn_facing_crosses_the_pi_seam_the_short_way` |
| Speed is the constant it claims | 3 | `walking_covers_the_speed_it_claims` in `pass/walk.rs`, predicted from `PLAYER_SPEED` rather than read back |
| Turn rate is the constant it claims | 3 | `turning_covers_the_rate_it_claims` in `pass/face.rs`, same discipline |
| Ground + horde + player fit one buffer | 3 | unit test in `sim` at the largest horde the clamp allows |
| The camera basis agrees with the projection | 3 | unit tests in `gfx/camera.rs` |
| Camera smoothing is frame-rate independent | 3 | `damp` uses `2^(-dt/half_life)`; unit test in `gfx/camera.rs` |
| The camera target cannot be set unsmoothed | 0 | private field; `follow` and `snap_to` are its only writers, and `snap_to` is deliberate |
| The camera never overshoots or bobs vertically | 3 | unit tests in `gfx/camera.rs` |
| Turning takes the short way round the ±PI seam | 3 | `shortest_arc` wraps the *difference*; unit test in `sim` |
| Turning is frame-rate independent and never overshoots | 3 | step clamped to the remaining arc; unit tests in `sim` |
| Facing cannot drift toward the precision limit | 3 | `angle::wrap` after every turn; unit tests in `sim/angle.rs` |
| Spawning does not swoop the camera in from the origin | 0 | `snap_to`, called in `resumed` before the first frame |
| A skipped frame is never counted as a rendered one | 1 | `Renderer::render` is `#[must_use]`, so ignoring the result is a denied warning |
| Readback rows respect the copy alignment | 3 | `padded_bytes_per_row`; unit test in `gfx/capture.rs` |
| A malformed harness command is reported, not ignored | 3 | `parse` returns `Result`; unit test in `app/harness.rs` |
| A screenshot that never happened is not reported as `ok` | 3 | `capture_has_stalled`; unit tests in `app/app.rs` |
| A body's position cannot leave the ground plane | 0 | positions are `Vec2`; `on_ground` is the only lift |
| An `EntityId` cannot be forged | 0 | private fields, no public constructor; `Slots::insert` is the only mint |
| A retired name cannot resolve to the body that took its row | 3 | generation bumped in `Slots::remove`; `a_despawned_name_stays_dead`, mutation-checked |
| A zeroed `EntityId` names nothing | 1 | `const _: () = assert!(FIRST_GENERATION > 0)`; `a_zeroed_id_names_nothing` |
| A despawn cannot desync the parallel arrays | 3 | `Slots::remove` returns the one index to `swap_remove`; `debug_assert` on every length in `Bodies::spawn`/`despawn` |
| A spawned body never streaks on its first frame | 0 | `Bodies::spawn` seeds `prev_pos` to the spawn point; it is the only door in |
| Spawn history reaches the determinism hash | 1 | `World::hash` destructures `Bodies`, and `Slots::hash` destructures itself |
| Nothing spawns already overlapping | 1 | `const _: () = assert!(ENEMY_SPACING > 2.0 * ENEMY_RADIUS)` |
| Coincident bodies separate deterministically, not into NaN | 3 | `escape_direction`; unit tests in `sim/contact.rs` |
| A contact normal is unit length | 0 | private fields on `Contact`; `contact::between` is the only constructor |
| A settled crowd stops reporting contacts | 3 | `SLOP` in `contact.rs`; `a_pair_resting_a_whisker_inside_touching_is_not_a_contact` |
| The contact slop stays above the arena's noise floor | 1 | const assert derived from `ARENA_HALF`, not a copied literal |
| A clamp cannot launder a NaN into a legal position | 3 | `debug_assert` in `pass::contain` naming the cause; `POISONED` |
| A scenario cannot end with a poisoned position | 3 | `check_finite` runs unconditionally, like `check_replay` |
| Every enemy pair is resolved once, not twice | 3 | `crowd` walks `j > i` via `split_at_mut`; `an_overlapped_pair_settles_at_touching` |
| A recycled slot does not inherit a behaviour | 3 | `Members::index` compares the whole id, not the slot; `a_recycled_slot_does_not_inherit_the_behaviour` |
| A behaviour costs its membership, not the horde | 0 | `pass::seek` iterates `Members::ids`, and is handed no way to reach a non-member |
| Structural changes cannot move a row underneath an ordinary pass | 0 | `spawn::drain` adds before row readers; attacks receive immutable body slices plus restricted sinks; `health::remove_defeated` removes only after every row-reading pass |
| A request cannot be granted late | 3 | `a_request_becomes_a_body_on_the_next_tick_and_not_before`, and `a_queued_spawn_lands_on_its_own_tick` pins the tick by position, mutation-checked |
| A refused spawn cannot be silent | 3 | `request_spawn` is `#[must_use]`; `Event::Refused` is emitted by the drain; `a_full_queue_refuses_out_loud` |
| A template grants what it names and nothing else | 3 | one line per behaviour in `pass::spawn::place`; `two_kinds_of_enemy_from_one_description`, whose bystander is the control |
| Pending requests reach the determinism hash | 1 | `World::hash` destructures `queue`; `SpawnQueue::hash` and `Template::hash_into` destructure themselves |
| The code that decides a spawn cannot perform one | 0 | `pass::source::trigger` is handed the queue and two facts, and no storage at all |
| A source fires on the ticks its cadence claims | 3 | `a_source_fires_on_the_ticks_it_promises`, whose golden trace is the tick list; mutation-checked against `every` vs `every - 1` |
| A gated source does not lose its turn | 3 | cadence is checked before the condition and a shut gate leaves the countdown alone; `a_source_waits_for_the_player_to_arrive` with `every: 60` inside a 40-tick budget, mutation-checked |
| A removed source stops making bodies | 3 | `removing_a_source_stops_the_flow`, whose budget covers two ticks it would otherwise have fired on |
| A source id can never be recycled | 0 | `Sources` is never compacted; a removed row stays `None`, which is why `SourceId` needs no generation |
| A ring never stacks what it makes | 3 | golden-angle step per emission; `a_ring_does_not_stack_what_it_makes` |
| Source state reaches the determinism hash | 1 | `World::hash` destructures `sources`; `Sources::hash` walks dead rows too, so ids line up on replay |
| The horde can always be outrun | 1 | `const _: () = assert!(SPEED < PLAYER_SPEED)` in `pass/seek.rs` |
| A chaser arrives without grinding | 3 | step clamped to `remaining`; `a_seeker_stops_where_it_touches`, whose golden trace is empty |
| A hitbox cannot move what it touches | 0 | `pass::attack` takes `&[Vec2]`; there is no `&mut` to write through |
| The hitbox window is exactly the configured active ticks | 3 | `is_active` is the single definition of both edges; default and tuned windows have golden traces |
| A swing connects once per body, not once per tick | 3 | `Attack::struck`; scenarios hold bodies inside multi-tick windows and assert one hit each |
| An attack cannot despawn while iterating bodies | 0 | `DamageSink` exposes only one-hit damage; it cannot reach `Bodies`, and zero-health rows wait for the final health pass |
| An enemy survives two hits and the third retires its identity | 3 | `three_hits_defeat_an_enemy` asserts health 2, health 1, then `alive: false`; its trace pins damage and removal to the confirmed hit ticks |
| Removing one health row cannot transfer damage to the swapped survivor | 3 | `despawning_preserves_a_swapped_survivors_payloads` exercises the real body-removal door with unequal remaining health |
| A swing cannot be interrupted or stacked | 3 | `a_press_during_a_swing_is_dropped`, which pins the default duration from both sides |
| The swing's phase cannot disagree with its timer | 0 | one `Option<InFlight>`; every phase is derived from it |
| Resolved attack values stay finite, nonzero where required, and bounded | 0 | `ResolvedAttack` fields are private; `AttackShape::try_new` and `ResolvedAttack::try_new` validate a complete value atomically. `RecoveryTicks` retains its serde validator for runtime recovery changes |
| Profile changes preserve the committed swing | 3 | `InFlight` owns the authored profile and complete resolved configuration; `attack_profiles_apply_to_the_next_swing` changes profiles during startup and asserts both swings and the golden trace |
| Every configured and committed attack profile reaches replay state | 3 | exhaustive `ResolvedAttack::hash` plus `every_configured_and_committed_attack_profile_reaches_the_hash` |
| Drawing the panel cannot mutate the world or consume an edit | 0 | `hud::draw` takes an `AttackStatus` snapshot and a shared `Menu`; it receives no mutable control or simulation reference |
| Menu transitions cannot leak gameplay input | 3 | `modal_transitions_discard_edges_and_require_fresh_gameplay_presses`; native and harness input both enter `Controls::on_key` |
| A hitbox swings where the character faces | 3 | `pass::attack` runs after `pass::face`; the yaw convention is mutation-checked |
| A swing is drawn where it is struck | 3 | one stored `Hitbox`; `pass::attack` and `World::extract` both *place* its discs rather than computing a position, so there is no second formula to drift. `the_swing_is_drawn_where_it_strikes` |
| A hitbox turns with the player | 3 | `Disc` is polar, so placing it is `facing + angle`. `the_hitbox_follows_the_facing`, mutation-checked against dropping the facing |
| An arc sweeps rather than cutting its chord | 3 | angle and distance interpolate separately in `Swing::generate`; `an_arc_sweeps_rather_than_cutting_the_chord` |
| A swing has a direction to point and a radius to hit with | 0 | private `AttackShape` fields and its atomic constructor prevent zero endpoints and non-positive radii before `Swing::new` is called |
| A swing's hitbox cannot outlive or precede its timer | 0 | `Attack` holds one `Option<InFlight>`; there is no way to read a shape without the tick that says which part of it is live |
| A swing leaves no gap for a body to pass through | 0 | `ResolvedAttack::try_new` rejects a complete path whose consecutive discs do not cover an enemy centre; the runtime calculation is necessary because the bound needs `atan2` and `sqrt` |
| The instance budget reserves room for a live swing | 1 | `PLAYER_INSTANCES` derives from the maximum active duration through `pass::attack::HITBOX_SAMPLES`; `a_full_horde_still_fits_alongside_the_ground_and_the_player` |
| Attack state reaches the determinism hash | 1 | `World::hash` destructures `Player`, and `Attack::hash` destructures itself |
| Behaviour membership reaches the determinism hash | 1 | `World::hash` destructures `seekers`; `Members::hash` destructures itself |
| A retired name can never be reused | 0 | `Slots::truncate` retires removed slots rather than resetting generations; there is no path that restarts a generation |
| A bulk respawn cannot leave a behaviour attached | 3 | `a_respawn_revokes_every_behaviour`, which caught a real resurrection |
| A new behaviour cannot be forgotten at spawn or despawn | 1 | `set_enemy_count` and `despawn_enemy` destructure `Self` exhaustively — a new field is E0027 |
| Asking whether two bodies touch cannot move them | 0 | `contact::between` takes `Vec2` by value and returns a `Contact` |
| The harness cannot exist unless asked for | 0 | `harness::start` returns `None` without `ARPG_HARNESS` |
| The harness cannot fall behind the bindings | 0 | key names live *in* `BINDINGS`; there is no second table to forget |
| A state report stays parseable when a value is not a number | 3 | `Report::number` writes `null`; `a_value_that_is_not_a_number_is_null` |
| The readable schedule cannot silently disagree with `World::step` | 4 | prose in `pass/mod.rs` — the one ordering claim nothing checks |
| Physical velocity cannot be overwritten by attack | 0 | `ImpulseSink` exposes only impulse submission; physical payload fields are private |
| Invalid impulse values are rejected by every reader | 0 | `Impulse` has private data and a validating `TryFrom`; serde uses that same constructor |
| Collision response receives a unit normal | 0 | `Physics::collide` takes a `Contact`, whose only constructor guarantees its normal |
| Overlap correction cannot inject momentum | 3 | `overlap_does_not_create_momentum`; projection and velocity response are separate operations |
| Momentum reaches a body outside the hitbox | 3 | `a_swing_pushes_a_body_it_did_not_hit` asserts the neighbour's position, velocity and trace |
| A hit changes velocity before it changes position | 3 | checkpoints in `the_hitbox_opens_and_shuts_on_schedule` cover the hit tick and subsequent movement |
| Physical payload survives row changes and cannot leak to a recycled id | 3 | `despawning_preserves_a_swapped_survivors_payloads` |
| Physics stays within the headless tick budget | 3 | `physics_stays_within_tick_budget` asserts mean time inside `World::step` |
| Scene load failure cannot partially replace a playtest | 3 | validation and capacity admission precede mutation; `invalid_and_over_capacity_scenes_leave_everything_untouched` and `rejected_replacement_preserves_the_current_playtest` |
| A scene's source descendants inherit its lifetime | 3 | owner carried through sources and spawn requests; `scenes_own_their_sources_and_descendants` asserts eviction and subsequent non-emission |
| A retired scene cannot be recreated by queued work | 3 | eager queue cancellation plus live-owner check at drain; `stale_producer_cannot_resurrect_an_evicted_scene` |
| A scene instance id is never reused within a world | 0 | private `SceneId`, monotonic checked allocation; records may be removed without rewinding the allocator |
| A fresh playtest cannot inherit player or pending input state | 3 | `restart_resets_the_complete_playtest_boundary` and `restart_cancels_old_delayed_actions_and_capture_replies` |
| Scene ownership and pending ownership reach replay state | 3 | exhaustive hashes and `scene_state_and_pending_ownership_reach_the_hash` |
| Authored names cannot corrupt the state protocol | 0 | `Report::text` and key encoding escape JSON strings at the write door |
