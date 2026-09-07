//! Tests for the world itself: determinism, the extract seam, and the budget.
//!
//! Split from `lib.rs` so that file stays the shape it describes — storage, the
//! two seams, and the schedule — rather than being mostly test code.

use super::*;
use arpg_core::{InstanceBuffer, Intent, MoveDir};

/// Counts allocations made on the calling thread.
///
/// Per-thread rather than a single global counter, and that is the whole trick:
/// `cargo test` runs tests in parallel, so a global count would be measuring
/// every other test's allocations too. The assertion would then fail at random,
/// which is the surest way to get a test deleted.
///
/// The thread-local is `const`-initialised so that first touch does not
/// allocate — an allocating allocator recurses into itself.
pub(crate) mod alloc_counter {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<u64> = const { Cell::new(0) };
    }

    pub(crate) struct Counting;

    #[expect(
        unsafe_code,
        reason = "GlobalAlloc is an unsafe trait by definition; this is the opt-out the \
                  workspace lint was set to `deny` rather than `forbid` to allow"
    )]
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // `try_with`, not `with`: during thread teardown the local is gone,
            // and a panic inside the allocator aborts the process.
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    /// How many allocations `f` made on this thread.
    pub(super) fn allocations(f: impl FnOnce()) -> u64 {
        let before = COUNT.with(Cell::get);
        f();
        COUNT.with(Cell::get).wrapping_sub(before)
    }
}

/// One tick's `Dt`.
///
/// Goes through a real [`Accumulator`], because there is no other route —
/// not even in here. A `#[cfg(test)]` back door would have been one line
/// and would have quietly made the invariant "no variable timestep, except
/// in the tests that define what correct means".
fn tick_dt() -> Dt {
    Accumulator::default().pending(Dt::SECS).next().expect("one tick's worth buys one tick")
}

/// Steps a fresh world `ticks` times and returns the hash after each one.
///
/// The sequence, not the final value: two runs that end up in the same
/// place having taken different routes are still a divergence, and a final
/// -state comparison calls them equal.
fn hash_sequence(enemies: usize, ticks: usize, dir_at: impl Fn(u64) -> MoveDir) -> Vec<u64> {
    let mut world = World::default();
    world.set_enemy_count(enemies);

    let mut acc = Accumulator::default();
    let mut seq = Vec::with_capacity(ticks);

    while seq.len() < ticks {
        for dt in acc.pending(Dt::SECS) {
            let dir = dir_at(world.tick());
            world.step(dt, Intent::new(dir, false));
            seq.push(world.hash());
        }
    }
    seq
}

/// A world whose player is clear of every body, so a test can measure
/// movement without measuring contact.
///
/// The horde spawns centred on the origin and so does the player, so the
/// two are in contact on the very first tick. Anything asking about
/// *movement* has to get out of the crowd first, or it is really asking
/// about the solver.
fn in_open_ground() -> World {
    let mut world = World::default();
    world.set_enemy_count(1);

    // The lone body spawns on top of the player. Two seconds east at
    // `PLAYER_SPEED` clears it by 18 units.
    for _ in 0..120 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    }
    world
}

/// **What the fixed timestep exists for.** Feed one input stream at
/// different frame rates; the simulation must not be able to tell.
///
/// It compares a hash of *all* state after *every* tick, so a divergence is
/// reported at the tick it happened rather than as two final positions that
/// happen to differ. And it runs in a crowd: contacts are resolved once per
/// tick, so under a variable timestep a faster machine resolves more of them
/// and the horde behaves differently. That is the bill the fixed timestep
/// pays, and this is the receipt.
#[test]
fn frame_rate_cannot_change_the_simulation() {
    // Powers of two, so every delta is exact in binary floating point: the
    // claim under test is about the accumulator, not about whether a third
    // of a tick rounds. Capped by `MAX_TICKS_PER_FRAME`, so 8 would silently
    // measure the stall path instead.
    let at = |ticks_per_frame: usize| {
        let mut world = World::default();
        world.set_enemy_count(64);

        let mut acc = Accumulator::default();
        let mut seq = Vec::new();

        for _ in 0..(120 / ticks_per_frame) {
            for dt in acc.pending(Dt::SECS * ticks_per_frame as f32) {
                world.step(dt, Intent::new(MoveDir::new(Vec3::X), false));
                seq.push(world.hash());
            }
        }
        seq
    };

    let reference = at(1);
    assert_eq!(reference.len(), 120, "the reference run did not take the ticks it was given");

    for n in [2, 4] {
        let other = at(n);
        assert_eq!(other.len(), reference.len(), "{n} ticks per frame ran a different number");

        let diverged = reference.iter().zip(&other).position(|(a, b)| a != b);
        assert_eq!(diverged, None, "{n} ticks per frame diverged at tick {diverged:?}");
    }
}

/// Determinism itself: the same run twice, compared tick by tick.
///
/// The frame schedule is deliberately ragged — the shape a real machine
/// produces, and the shape a variable timestep leaks through.
/// A wholesale respawn retires every name, so any behaviour still attached
/// to one would be attached to whoever inherits it. Asserted rather than
/// left to the ordering comment in `set_enemy_count`, because the failure —
/// a body nobody asked to chase, chasing — is silent.
#[test]
fn a_respawn_revokes_every_behaviour() {
    let mut world = World::default();
    world.set_enemy_count(0);

    let id = world.place(Vec2::new(3.0, 0.0), Template::BODY).expect("room for one body");
    assert!(world.add_seek(id));
    assert_eq!(world.seeker_count(), 1);

    world.set_enemy_count(4);

    assert_eq!(world.seeker_count(), 0, "a behaviour survived the respawn that retired its name");
    assert!(!world.is_alive(id), "the old name outlived the horde it belonged to");
}
#[test]
fn one_input_stream_replays_to_the_same_hash_every_tick() {
    let ragged = [0.004, 0.019, 0.016_1, 0.033, 0.000_9, 0.017_2];

    let run = || {
        let mut world = World::default();
        world.set_enemy_count(256);

        let mut acc = Accumulator::default();
        let mut seq = Vec::new();

        for (i, &frame) in ragged.iter().cycle().take(300).enumerate() {
            // Something that keeps turning, so facing is under test too.
            let dir = MoveDir::new(if i % 40 < 20 { Vec3::X } else { Vec3::NEG_Z });
            for dt in acc.pending(frame) {
                world.step(dt, Intent::new(dir, false));
                seq.push(world.hash());
            }
        }
        seq
    };

    let first = run();
    assert!(first.len() > 100, "the schedule ran only {} ticks", first.len());
    assert_eq!(first, run(), "two identical runs disagreed");
}

/// **The sensitivity check, and it is not optional.** A `hash()` that
/// returned a constant would pass both tests above and every replay
/// scenario ever written against it. So: two streams that agree until tick
/// 60 must hash identically up to there and differ from there on.
#[test]
fn the_hash_localises_where_two_streams_diverge() {
    let east = MoveDir::new(Vec3::X);
    let north = MoveDir::new(Vec3::NEG_Z);
    const SPLIT: u64 = 60;

    let straight = hash_sequence(32, 120, |_| east);
    let turning = hash_sequence(32, 120, |t| if t < SPLIT { east } else { north });

    let split = SPLIT as usize;
    assert_eq!(straight[..split], turning[..split], "streams differed before they differed");
    assert_ne!(straight[split], turning[split], "the first differing tick hashed the same");
    assert_ne!(straight.last(), turning.last(), "the divergence washed out");
}

/// Where every enemy was drawn. `extract` pushes ground, the horde, any live
/// swing, then the player, so the horde starts right after the floor.
fn drawn_enemies(world: &World, alpha: Alpha, buffer: &mut InstanceBuffer) -> Vec<Vec3> {
    world.extract(alpha, buffer.sink());
    buffer.as_slice()[GROUND_INSTANCES..][..world.enemy_count()]
        .iter()
        .map(Instance::pos)
        .collect()
}

/// Puts the player inside the horde and walks, so the solver is displacing
/// bodies every tick. Anything asking whether the *horde* is drawn right
/// has to be measured somewhere the horde actually moves — in open ground
/// every body sits still and a broken blend is indistinguishable from a
/// working one.
fn shoving_through_the_crowd(enemies: usize) -> World {
    let mut world = World::default();
    world.set_enemy_count(enemies);

    // Only a few ticks: at `PLAYER_SPEED` the player clears a small horde
    // in well under a second, and then contacts drop to zero and this
    // measures open ground again. 20 ticks is 3.0 units, which walks
    // straight out of a 64-body crowd (5.6 units across).
    for _ in 0..5 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    }
    assert!(world.contacts() > 0, "nothing is in contact, so nothing is being pushed");
    world
}

/// Reads the position of the last instance a sink was given — the player,
/// since `extract` pushes it last.
fn drawn_player(world: &World, alpha: Alpha, buffer: &mut InstanceBuffer) -> Vec3 {
    world.extract(alpha, buffer.sink());
    buffer.as_slice().last().expect("extract pushes at least the player").pos()
}

/// **The gate for render interpolation.** The endpoints have to be exact,
/// or the blend is drawing something the simulation never believed.
#[test]
fn the_blend_endpoints_are_the_two_ticks_themselves() {
    let mut world = in_open_ground();
    world.set_enemy_count(4);
    let mut buffer = InstanceBuffer::default();

    let before = world.player_pos();
    world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    let after = world.player_pos();
    assert_ne!(before, after, "the tick under test did not move anything");

    assert_eq!(drawn_player(&world, Alpha::ZERO, &mut buffer), before, "alpha 0 is not the previous tick");
    assert_eq!(drawn_player(&world, Alpha::ONE, &mut buffer), after, "alpha 1 is not the current tick");
}

/// Between the endpoints it has to actually be *between*, and monotonic —
/// a blend that jumps or backtracks is judder wearing a different hat.
#[test]
fn the_blend_crosses_the_gap_once_and_in_order() {
    let mut world = in_open_ground();
    world.set_enemy_count(4);
    let mut buffer = InstanceBuffer::default();

    let before = world.player_pos();
    world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    let after = world.player_pos();
    let span = (after - before).length();

    // Nine tenths, then the endpoint. An accumulator *cannot* produce alpha
    // 1: a full tick's worth of carry is a tick, not a blend, so `pending`
    // consumes it and leaves zero behind. That is why `Alpha::ONE` is a
    // constant rather than something a frame ever asks for, and asking for
    // it here by feeding a whole tick would silently sample alpha 0 again.
    let sampled = (0..10).map(|i| {
        let mut acc = Accumulator::default();
        acc.pending(Dt::SECS * i as f32 / 10.0);
        acc.alpha()
    });

    let mut furthest = -1.0;

    for alpha in sampled.chain(core::iter::once(Alpha::ONE)) {
        let drawn = drawn_player(&world, alpha, &mut buffer);
        let a = alpha.get();

        // On the segment: the two legs sum to the whole only for a point
        // between the ends.
        let off = (drawn - before).length() + (after - drawn).length() - span;
        assert!(off.abs() < 1e-4, "the drawn position left the segment at alpha {a} by {off}");

        // And moving forward along it, never back.
        let progress = (drawn - before).length();
        assert!(progress >= furthest - 1e-6, "the blend went backwards at alpha {a}");
        furthest = progress;
    }

    assert!((furthest - span).abs() < 1e-4, "the blend reached {furthest}, the tick moved {span}");
}

/// **Found by mutation.** Every other blend test here reads the player,
/// because the player is the last instance and therefore the easy one. So
/// three separate breakages of the *horde's* interpolation — not blending
/// it at all, blending it backwards, and never recording where it was —
/// passed the entire suite. The horde is a thousand of the bodies on screen
/// and one of them was being checked.
#[test]
fn the_horde_is_interpolated_too() {
    let mut world = shoving_through_the_crowd(256);
    let mut buffer = InstanceBuffer::default();

    let before: Vec<Vec2> = world.enemies.pos.clone();
    world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    let after: Vec<Vec2> = world.enemies.pos.clone();

    let moved: Vec<usize> =
        (0..after.len()).filter(|&i| before[i] != after[i]).collect();
    assert!(!moved.is_empty(), "no body moved during the tick under test");

    let at_zero = drawn_enemies(&world, Alpha::ZERO, &mut buffer);
    let at_one = drawn_enemies(&world, Alpha::ONE, &mut buffer);
    let at_half = drawn_enemies(&world, half(), &mut buffer);

    for i in 0..after.len() {
        assert_eq!(at_zero[i], on_ground(before[i], ENEMY_HALF_HEIGHT), "body {i} at alpha 0");
        assert_eq!(at_one[i], on_ground(after[i], ENEMY_HALF_HEIGHT), "body {i} at alpha 1");
    }

    for &i in &moved {
        let span = (at_one[i] - at_zero[i]).length();
        let off = (at_half[i] - at_zero[i]).length() + (at_one[i] - at_half[i]).length() - span;
        assert!(off.abs() < 1e-5, "body {i} left the segment between its two ticks");
        assert_ne!(at_half[i], at_zero[i], "body {i} did not move off its previous tick");
        assert_ne!(at_half[i], at_one[i], "body {i} was drawn already arrived");
    }
}

/// An empty arena has to work, not merely not crash: it is where a fight
/// ends, and it is the only setup in which a scenario can predict plain
/// movement without predicting the solver too.
#[test]
fn an_empty_horde_is_a_legal_world() {
    let mut world = World::default();
    world.set_enemy_count(0);
    assert_eq!(world.enemy_count(), 0);

    for _ in 0..30 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    }
    assert_eq!(world.contacts(), 0, "an empty arena reported a contact");

    // Movement is then exactly the constant, with nothing to interfere.
    let expected = 30.0 * pass::walk::PER_TICK;
    assert!((world.player_pos().x - expected).abs() < 1e-4);

    // And it still draws: ground plus the player, no horde.
    let mut buffer = InstanceBuffer::default();
    world.extract(Alpha::ONE, buffer.sink());
    assert_eq!(buffer.as_slice().len(), GROUND_INSTANCES + 1);
}

/// **Found by mutation, and it is a real artefact.** Every other test here
/// steps immediately after changing the horde, and `step` overwrites `prev`
/// — so a respawn that leaves a stale `prev` behind is invisible to all of
/// them.
///
/// It is visible on screen, though, and the fixed timestep is what makes it
/// so: uncapped at ~300fps most frames run **zero** ticks, so a frame is
/// drawn between `set_enemy_count` and the next `step` most of the time.
/// With a stale `prev` the whole horde streaks in from wherever the old one
/// stood — pressing `]` would flicker a thousand bodies across the arena.
#[test]
fn a_respawned_horde_is_drawn_standing_still() {
    let mut world = shoving_through_the_crowd(256);
    let mut buffer = InstanceBuffer::default();

    // Change the count and draw with no tick in between.
    world.set_enemy_count(64);

    let standing: Vec<Vec3> =
        world.enemies.pos.iter().map(|&p| on_ground(p, ENEMY_HALF_HEIGHT)).collect();

    for alpha in [Alpha::ZERO, half(), Alpha::ONE] {
        assert_eq!(
            drawn_enemies(&world, alpha, &mut buffer),
            standing,
            "a horde that has not been stepped was drawn mid-move at alpha {}",
            alpha.get()
        );
    }
}

/// **The rule that makes interpolation safe**, checked rather than assumed:
/// drawing must not change what the simulation believes. `extract` takes
/// `&self`, so this cannot fail without the signature changing — which is
/// the point, and is why the assertion is cheap enough to keep.
#[test]
fn drawing_never_touches_sim_state() {
    let mut world = World::default();
    world.set_enemy_count(64);
    for _ in 0..10 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    }

    let untouched = world.hash();
    let mut buffer = InstanceBuffer::default();

    for i in 0..=8 {
        let mut acc = Accumulator::default();
        acc.pending(Dt::SECS * i as f32 / 8.0);
        world.extract(acc.alpha(), buffer.sink());
        assert_eq!(world.hash(), untouched, "extract at {i}/8 of a tick changed the world");
    }
}

/// **The seam again, one layer up.** `facing` is wrapped to `-PI..=PI`, so
/// a body turning through south steps from `+3.13` to `-3.13` — a real turn
/// of 0.02 radians whose naive lerp spins it 6.26 the other way for exactly
/// one frame. That reads as a flicker, and flickers get blamed on the
/// renderer rather than on the maths.
#[test]
fn the_drawn_facing_crosses_the_pi_seam_the_short_way() {
    use core::f32::consts::PI;

    let from = PI - 0.01;
    let to = -PI + 0.01;

    let mid = blend_angle(from, to, half());
    assert!(
        mid.abs() > PI - 0.02,
        "the drawn facing took the long way round the seam: {mid} should be near ±PI"
    );

    // And the endpoints still land exactly where they should.
    assert!((blend_angle(from, to, Alpha::ZERO) - from).abs() < 1e-6);
    assert!((blend_angle(from, to, Alpha::ONE) - to).abs() < 1e-6);

    // Wrapped, so repeated blending cannot drift out of range.
    assert!(mid.abs() <= PI, "the drawn facing left -PI..=PI");
}

/// Half a tick in, minted the only way an `Alpha` can be.
fn half() -> Alpha {
    let mut acc = Accumulator::default();
    acc.pending(Dt::SECS / 2.0);
    acc.alpha()
}

/// A body nothing touched must be drawn where it stands, not streaked back
/// to wherever it last happened to move. This is what `remember()` being
/// called for *every* body, every tick, buys.
#[test]
fn a_body_that_did_not_move_is_drawn_where_it_is() {
    let mut world = in_open_ground();
    world.set_enemy_count(16);

    // Out in the open, standing still: nothing moves at all.
    for _ in 0..5 {
        world.step(tick_dt(), Intent::NONE);
    }
    assert_eq!(world.contacts(), 0, "something is touching, so this measures the solver");

    let mut buffer = InstanceBuffer::default();
    world.extract(half(), buffer.sink());
    let blended: Vec<Vec3> = buffer.as_slice().iter().map(Instance::pos).collect();

    world.extract(Alpha::ONE, buffer.sink());
    for (i, (a, b)) in blended.iter().zip(buffer.as_slice()).enumerate() {
        assert_eq!(*a, b.pos(), "instance {i} moved between alphas while nothing was moving");
    }
}

/// `prev` has to be *last tick's* value, not two ticks ago and not this
/// tick's. Nothing else in this file pins that: the sim is unaffected by a
/// missing `remember()`, so both runs of a replay would agree perfectly
/// while every body on screen streaked.
#[test]
fn the_previous_tick_is_the_previous_tick() {
    // In the crowd, not in open ground: out there no enemy ever moves, so
    // `prev_pos` trivially equals `pos` and skipping `remember()` entirely
    // passes. That is how the first version of this test was written, and
    // mutation is how it was caught.
    let mut world = shoving_through_the_crowd(256);
    let east = MoveDir::new(Vec3::X);

    for _ in 0..4 {
        let expected = world.player.pos;
        let enemies_before = world.enemies.pos.clone();

        world.step(tick_dt(), Intent::new(east, false));

        assert_eq!(world.player.prev_pos, expected, "the player's prev is not last tick");
        assert_eq!(world.enemies.prev_pos, enemies_before, "the horde's prev is not last tick");
    }
}

/// **Found by mutation, not by design.** Deleting the horde from `hash()`
/// entirely — `let _ = enemy_pos;` — passed every other test in this file,
/// including both replay gates. They only ever vary the *player's* input,
/// so the player's state carries the whole signal and a hash that sees
/// nothing else agrees with itself perfectly.
///
/// The destructuring in `hash()` catches a field nobody *binds*. This
/// catches a field bound and then dropped on the floor, which is what an
/// incomplete hash actually looks like when someone is refactoring.
#[test]
fn every_field_of_the_world_reaches_the_hash() {
    /// A field of `World` and the smallest change that touches it.
    type Poke = (&'static str, fn(&mut World));

    let fields: [Poke; 7] = [
        ("player.pos", |w| w.player.pos.x += 0.001),
        ("player.facing", |w| w.player.facing += 0.001),
        ("tick", |w| w.tick += 1),
        ("contacts", |w| w.contacts += 1),
        ("enemies.pos", |w| w.enemies.pos[0].x += 0.001),
        // A pending request is a body that exists in one of two otherwise
        // identical worlds one tick from now.
        ("queue", |w| assert!(w.request_spawn(Vec2::ZERO, Template::BODY))),
        // A source's own state decides when and where the next body appears,
        // so two worlds agreeing on every body still diverge from here.
        ("sources", |w| {
            w.add_source(Source::new(Placement::At(Vec2::ZERO), Template::BODY));
        }),
    ];

    for (field, poke) in fields {
        let mut world = World::default();
        let before = world.hash();
        poke(&mut world);
        assert_ne!(world.hash(), before, "{field} never reaches the hash");
    }
}

/// The horde is what the player's own state cannot stand in for: walking
/// through the crowd displaces bodies, and a replay that agrees on the
/// player while the horde drifts is the divergence that matters most once
/// enemies do anything on their own.
#[test]
fn the_horde_is_part_of_what_a_replay_compares() {
    let mut world = World::default();
    world.set_enemy_count(64);

    // Stand still. Only the solver moves anything, so any change in the
    // hash from here is the horde's.
    let settled = {
        for _ in 0..30 {
            world.step(tick_dt(), Intent::NONE);
        }
        world.hash()
    };

    world.enemies.pos[7] += Vec2::new(0.01, -0.01);
    assert_ne!(world.hash(), settled, "displacing a body left the hash unchanged");
}

/// A steady-state frame must not touch the allocator.
///
/// Not a micro-optimisation: an allocation in the tick path is a latency
/// spike with no fixed size, and frame pacing is the foundation every feel
/// mechanic here gets measured against. It is also the cheapest possible
/// guard against someone adding a `Vec` inside a pass, which is the natural
/// way to write a broadphase and the wrong way to run one.
///
/// Warmed up first, because the first tick of a fresh world is not a steady
/// state and asserting on it would measure spawning.
#[test]
fn a_steady_state_frame_allocates_nothing() {
    let east = MoveDir::new(Vec3::X);
    let dt = tick_dt();

    let mut world = World::default();
    world.set_enemy_count(512);
    let mut buffer = InstanceBuffer::default();

    world.step(dt, Intent::new(east, false));
    world.extract(Alpha::ONE, buffer.sink());

    let allocations = alloc_counter::allocations(|| {
        for tick in 0..60 {
            // **Swings included.** The guard used to pass `false` on every
            // tick, so the one allocating line the attack added — pushing a
            // struck body onto a list reserved for sixteen — was never
            // reached by the thing whose job is to notice. A wider hitbox
            // or a bigger body radius would have gone unremarked.
            //
            // Every 20 ticks is exactly the swing length, so this runs three
            // back-to-back swings rather than one and then idling.
            world.step(dt, Intent::new(east, tick % 20 == 0));
            world.extract(Alpha::ONE, buffer.sink());
        }
    });

    assert_eq!(allocations, 0, "60 steady-state frames allocated {allocations} times");
}

#[test]
fn no_input_does_not_move_the_player() {
    let mut world = in_open_ground();
    let start = world.player_pos();
    for _ in 0..60 {
        world.step(tick_dt(), Intent::NONE);
    }
    assert_eq!(world.player_pos(), start);
}

/// Walking into the wall must stop, not leave the ground plane — and must
/// stay finite, since a NaN position would silently vanish the character.
#[test]
fn the_player_cannot_walk_off_the_arena() {
    let mut world = World::default();
    for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
        // Twenty seconds at `PLAYER_SPEED` is 180 units — comfortably past
        // the far wall from anywhere in a 96-unit half-arena, so this
        // reaches the clamp rather than merely walking toward it.
        for _ in 0..1200 {
            world.step(tick_dt(), Intent::new(MoveDir::new(dir), false));
        }
        let pos = world.player_pos();
        assert!(pos.is_finite());
        assert!(pos.x.abs() <= ARENA_HALF && pos.z.abs() <= ARENA_HALF, "escaped: {pos}");
    }
}

/// **The seam that turning exists to get right.** Crossing the ±PI branch
/// cut must be a small step, not an almost-full revolution the other way.
#[test]
fn turning_takes_the_short_way_around() {
    let nearly_half_turn = std::f32::consts::PI - 0.1;
    let just_past = -nearly_half_turn;

    let arc = angle::shortest_arc(nearly_half_turn, just_past);
    assert!(arc.abs() < 0.3, "went the long way: {arc}");

    // And the naive subtraction this replaces really does get it wrong,
    // which is why the wrapping is not decoration.
    assert!((just_past - nearly_half_turn).abs() > 6.0);
}

#[test]
fn facing_follows_the_direction_of_travel() {
    let mut world = World::default();
    for _ in 0..120 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    }
    // atan2(dir.x, dir.z): due east is +X, so a quarter turn from +Z.
    assert!((world.player.facing - std::f32::consts::FRAC_PI_2).abs() < 1e-4);

    for _ in 0..120 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::Z), false));
    }
    assert!(world.player.facing.abs() < 1e-4, "should face +Z");
}

/// A fixed turn rate is only frame-rate independent if the step is clamped
/// to the remaining arc; without the clamp the coarse step overshoots and
/// the two disagree.
#[test]
fn turning_is_frame_rate_independent() {
    let west = MoveDir::new(Vec3::NEG_X);

    // One frame worth five ticks against five frames worth one, which is
    // the same comparison as before now that a frame cannot hand the sim
    // an arbitrary delta.
    let mut coarse = World::default();
    let mut coarse_acc = Accumulator::default();
    for dt in coarse_acc.pending(Dt::SECS * 5.0) {
        coarse.step(dt, Intent::new(west, false));
    }

    let mut fine = World::default();
    let mut fine_acc = Accumulator::default();
    for _ in 0..5 {
        for dt in fine_acc.pending(Dt::SECS) {
            fine.step(dt, Intent::new(west, false));
        }
    }

    assert_eq!(coarse.hash(), fine.hash());
}

#[test]
fn turning_never_overshoots_its_target() {
    let mut world = World::default();
    let target = std::f32::consts::FRAC_PI_2;

    for _ in 0..200 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
        assert!(world.player.facing >= 0.0);
        assert!(world.player.facing <= target, "overshot to {}", world.player.facing);
    }
}

/// Releasing the keys must not reorient the character — it would turn away
/// from whatever it just walked up to.
#[test]
fn standing_still_keeps_the_last_facing() {
    let mut world = World::default();
    for _ in 0..120 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::NEG_Z), false));
    }
    let settled = world.player.facing;

    for _ in 0..120 {
        world.step(tick_dt(), Intent::NONE);
    }
    assert_eq!(world.player.facing, settled);
}

/// Facing must stay canonical however long the session runs, rather than
/// accumulating toward the range where f32 loses angular precision.
#[test]
fn facing_stays_wrapped_while_spinning() {
    let mut world = World::default();
    let circle = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z];

    for lap in 0..50 {
        for _ in 0..30 {
            world.step(tick_dt(), Intent::new(MoveDir::new(circle[lap % 4]), false));
        }
        assert!(
            world.player.facing.abs() <= std::f32::consts::PI + 1e-6,
            "drifted to {}",
            world.player.facing
        );
    }
}

/// The whole world — floor, horde, player and a swing in the air — has to fit
/// the one buffer they share, at the largest horde the clamp permits.
///
/// The swing is counted rather than driven. Stepping a full horde would put
/// 180-odd thousand bodies through a brute-force solver, which is not a unit
/// test; and what needs checking is the *reservation*, not the drawing. So
/// this asserts the gap left by an idle player is exactly one swing wide,
/// which is the same statement and costs nothing.
#[test]
fn a_full_horde_still_fits_alongside_the_ground_and_the_player() {
    let mut world = World::default();
    world.set_enemy_count(usize::MAX);

    let mut buf = InstanceBuffer::default();
    world.extract(Alpha::ONE, buf.sink());

    assert!(!world.player.attack.is_swinging(), "a fresh world is not mid-swing");
    assert_eq!(buf.as_slice().len() + pass::attack::HITBOX_SAMPLES, MAX_INSTANCES);
}

/// **The property the whole design is for: the swing is drawn where it is
/// struck.**
///
/// Both halves are pinned, in the two places that can see them. The scenario
/// `the_hitbox_opens_and_shuts_on_schedule` puts a body at exactly `REACH`
/// dead ahead and asserts it is hit; this asserts the instance that comes out
/// of `extract` sits on that same point. A renderer that formed its own
/// opinion about where the sword is would pass one and fail the other.
///
/// Facing is deliberately non-zero. At facing 0 a mirrored placement is
/// indistinguishable from a correct one, and mirrored is exactly what a hitbox
/// built from a hand-written basis comes out as.
#[test]
fn the_swing_is_drawn_where_it_strikes() {
    let dt = tick_dt();
    let east = MoveDir::new(Vec3::X);

    let mut world = World::default();
    world.set_enemy_count(0);

    // Long enough for the turn to arrive and clamp: PLAYER_TURN_RATE covers
    // the quarter-turn in under seven ticks. See `walk_east`.
    for _ in 0..30 {
        world.step(dt, Intent::new(east, false));
    }

    world.step(dt, Intent::new(MoveDir::NONE, true));
    while !world.hitbox_is_live() {
        world.step(dt, Intent::NONE);
    }

    let mut buf = InstanceBuffer::default();
    world.extract(Alpha::ONE, buf.sink());

    // Ground, no horde, then the swing — which during the active window is
    // the live disc and nothing else, so there is exactly one of it.
    assert_eq!(
        buf.as_slice().len(),
        GROUND_INSTANCES + 2,
        "the active window should draw one disc, plus the player"
    );
    let drawn = buf.as_slice()[GROUND_INSTANCES].pos();
    let expected = world.player.pos + Vec2::new(pass::attack::REACH, 0.0);

    assert!(
        (drawn.x - expected.x).abs() < 1e-4 && (drawn.z - expected.y).abs() < 1e-4,
        "the swing drew at {drawn:?}, but strikes at {expected:?}"
    );
}

/// The count is derived from the storage, so asking for N must actually
/// produce N bodies — not N draw calls over a formula.
#[test]
fn the_horde_holds_exactly_the_requested_count() {
    let mut world = World::default();
    assert_eq!(world.enemy_count(), DEFAULT_ENEMIES);

    for n in [1, 17, 512, 1024, 4096] {
        world.set_enemy_count(n);
        assert_eq!(world.enemy_count(), n);
        assert_eq!(world.enemies.pos.len(), n);
    }
}

/// Nothing may spawn already overlapping.
///
/// The const assert beside `ENEMY_SPACING` covers the constants; this
/// covers the *layout* they produce, which is the thing that actually has
/// to hold. Once bodies push each other apart, an interpenetrated spawn
/// resolves every overlap on frame one and detonates the horde — a failure
/// that looks like a physics bug and is a spawning bug.
#[test]
fn the_horde_spawns_with_a_gap_between_every_body() {
    let mut world = World::default();
    world.set_enemy_count(1024);

    let pos = &world.enemies.pos;
    let mut closest = f32::MAX;
    for i in 0..pos.len() {
        for j in i + 1..pos.len() {
            closest = closest.min(pos[i].distance(pos[j]));
        }
    }

    assert!(
        closest > ENEMY_SCALE.x,
        "spawned {closest} apart, but a body is {} wide",
        ENEMY_SCALE.x
    );
}

/// The grid is centred on the origin, which is what puts the player inside
/// the horde rather than beside it.
///
/// Exactly centred only when N is a perfect square. Otherwise the last row
/// is partial and drags the centroid by up to one spacing — which is the
/// real behaviour and worth pinning at that bound rather than pretending
/// the grid is always square.
#[test]
fn the_horde_is_centred_on_the_origin() {
    let mut world = World::default();

    let centroid_at = |world: &World| {
        let pos = &world.enemies.pos;
        pos.iter().fold(Vec2::ZERO, |acc, &p| acc + p) / pos.len() as f32
    };

    for n in [1, 4, 1024] {
        world.set_enemy_count(n);
        let c = centroid_at(&world);
        assert!(c.length() < 1e-3, "square N={n} should be exactly centred, got {c}");
    }

    for n in [17, 500, 4095] {
        world.set_enemy_count(n);
        let c = centroid_at(&world);
        assert!(c.length() < ENEMY_SPACING, "ragged N={n} drifted {c}, more than one row");
    }
}

/// The one place `Vec2::y` means world Z, so it is worth pinning: a
/// transposition here is horizontal either way and would draw the whole
/// horde mirrored along a diagonal without a single test failing elsewhere.
#[test]
fn a_ground_position_keeps_x_and_lifts_y_into_z() {
    assert_eq!(on_ground(Vec2::new(3.0, -7.0), 0.25), Vec3::new(3.0, 0.25, -7.0));
}

/// **The invariant the pass exists to establish**, checked on the real
/// world rather than on a pair: after a step, nothing is inside the player.
#[test]
fn no_enemy_is_left_overlapping_the_player() {
    let mut world = World::default();

    // Walk into the middle of the horde and keep going.
    for _ in 0..240 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
    }

    let contact = PLAYER_RADIUS + ENEMY_RADIUS;
    let player = world.player.pos;
    for (i, &enemy) in world.enemies.pos.iter().enumerate() {
        let gap = player.distance(enemy);
        assert!(gap >= contact - 1e-4, "enemy {i} is {gap} from the player, needs {contact}");
    }
}

/// Walking through the horde must displace it. A player that leaves the
/// crowd exactly as it found it is not colliding with anything, which is a
/// failure the previous test cannot see — it passes trivially if nothing
/// ever overlaps because nothing ever touches.
#[test]
fn walking_through_the_horde_displaces_it() {
    let mut world = World::default();
    let before = world.enemies.pos.clone();

    let mut ever_touched = 0;
    for _ in 0..240 {
        world.step(tick_dt(), Intent::new(MoveDir::new(Vec3::X), false));
        ever_touched += world.contacts();
    }

    let moved = before
        .iter()
        .zip(&world.enemies.pos)
        .filter(|(a, b)| a.distance(**b) > 1e-4)
        .count();

    assert!(ever_touched > 0, "nothing was ever in contact");
    assert!(moved > 0, "the player walked straight through {} bodies", before.len());
}

/// The contact count has to mean something, or it is a comforting number
/// that would keep reporting zero if the solver stopped working. Standing
/// clear of everything is zero; standing inside the horde is not.
#[test]
fn the_contact_count_tracks_whether_anything_is_touching() {
    let mut clear = in_open_ground();
    clear.step(tick_dt(), Intent::NONE);
    assert_eq!(clear.contacts(), 0, "nothing is near the player out here");

    // The horde is centred on the origin and so is the player, so the
    // spawn itself puts bodies in contact.
    let mut crowded = World::default();
    crowded.step(tick_dt(), Intent::NONE);
    assert!(crowded.contacts() > 0, "spawned inside the horde and touched nothing");
}

/// Everything stays inside the world, including bodies that only moved
/// because something shoved them.
#[test]
fn nothing_is_pushed_out_of_the_arena() {
    let mut world = World::default();
    world.set_enemy_count(256);

    for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
        for _ in 0..600 {
            world.step(tick_dt(), Intent::new(MoveDir::new(dir), false));
        }
        for &enemy in &world.enemies.pos {
            assert!(enemy.is_finite(), "poisoned position {enemy}");
            assert!(
                enemy.x.abs() <= ARENA_HALF && enemy.y.abs() <= ARENA_HALF,
                "escaped to {enemy}"
            );
        }
    }
}

/// **The latency the queue promises, stated as a test.** A request made between
/// ticks is granted by the first pass of the next tick — so the body does not
/// exist while the request is pending, and does exist the moment one tick has
/// run. Both halves matter: a drain that ran late would still pass the second
/// assertion a tick later, and nothing else here would notice.
#[test]
fn a_request_becomes_a_body_on_the_next_tick_and_not_before() {
    let mut world = World::default();
    world.set_enemy_count(0);

    assert!(world.request_spawn(Vec2::new(3.0, 0.0), Template::BODY));
    assert_eq!(world.enemy_count(), 0, "the request was granted without a tick running");

    world.step(tick_dt(), Intent::NONE);
    assert_eq!(world.enemy_count(), 1);
}

/// The template is the list of behaviours the new body is granted, and the
/// control is the body granted none. A drain that granted everything to
/// everybody would satisfy every assertion about the first.
#[test]
fn a_template_grants_what_it_names_and_nothing_else() {
    let mut world = World::default();
    world.set_enemy_count(0);

    assert!(world.request_spawn(Vec2::new(3.0, 0.0), Template::BODY.seeking()));
    assert!(world.request_spawn(Vec2::new(-3.0, 0.0), Template::BODY));
    world.step(tick_dt(), Intent::NONE);

    assert_eq!(world.enemy_count(), 2);
    assert_eq!(world.seeker_count(), 1, "the template granted the wrong number of behaviours");
}

/// **A dropped spawn must not be silent.** Nothing appears, nothing errors, and
/// the only other evidence would be a body somebody expected and did not get —
/// so the refusal is a return value at the door and an event in the trace.
#[test]
fn a_full_queue_refuses_out_loud() {
    use crate::pass::spawn::QUEUE_CAPACITY;

    let mut world = World::default();
    world.set_enemy_count(0);
    world.clear_trace();

    for i in 0..QUEUE_CAPACITY {
        assert!(world.request_spawn(Vec2::ZERO, Template::BODY), "request {i} refused early");
    }
    assert!(!world.request_spawn(Vec2::ZERO, Template::BODY), "the queue accepted past capacity");

    world.step(tick_dt(), Intent::NONE);

    assert_eq!(world.enemy_count(), QUEUE_CAPACITY, "the accepted requests were not all granted");
    let refusals: Vec<_> = world
        .trace()
        .iter()
        .filter_map(|(_, e)| match e {
            Event::Refused { count } => Some(count),
            _ => None,
        })
        .collect();
    assert_eq!(refusals, vec![1], "the dropped request left no trace of itself");
}

/// A wholesale respawn is a reset, and a request decided before it must not
/// land a body after it. The dial is the one door the horde's size goes
/// through; a queue that survived it would put that size back out of date one
/// tick later.
#[test]
fn resetting_the_horde_forgets_what_was_pending() {
    let mut world = World::default();
    world.set_enemy_count(0);

    assert!(world.request_spawn(Vec2::new(3.0, 0.0), Template::BODY));
    world.set_enemy_count(4);
    world.step(tick_dt(), Intent::NONE);

    assert_eq!(world.enemy_count(), 4, "a request from before the reset was granted after it");
}

/// A source at a stated cadence puts bodies in on the ticks it promises: the
/// first immediately, then one every `every` ticks.
///
/// **Starting ready is the decision being pinned here.** The alternative — wait
/// one cadence, then fire — puts the first body of every level at a time nobody
/// wrote down, and reads as a source that failed to start.
#[test]
fn a_source_fires_on_the_cadence_it_promises() {
    let mut world = World::default();
    world.set_enemy_count(0);
    world.add_source(Source::new(Placement::At(Vec2::new(30.0, 0.0)), Template::BODY).every(10));

    // Ticks 0, 10, 20 and 30 fire; 31 ticks is one past the fourth.
    for _ in 0..31 {
        world.step(tick_dt(), Intent::NONE);
    }

    assert_eq!(world.enemy_count(), 4, "a source at every=10 over 31 ticks");
}

/// **Ready-and-waiting.** A source whose cadence has come round but whose
/// condition is shut does not lose its turn: it fires on the first tick the
/// condition opens, not on the next multiple of the cadence after it.
///
/// That is the behaviour a room full of enemies wants — they appear when the
/// player walks in, rather than up to `every` ticks later — and it is invisible
/// to any test that only counts bodies at the end of a run.
#[test]
fn a_held_back_source_fires_the_instant_its_condition_opens() {
    let mut world = World::default();
    world.set_enemy_count(0);

    let far = Vec2::new(30.0, 0.0);
    world.add_source(
        Source::new(Placement::At(far), Template::BODY)
            .every(60)
            .when(Condition::PlayerWithin(5.0)),
    );

    // Long enough that a source counting down regardless would have come round
    // twice over.
    for _ in 0..120 {
        world.step(tick_dt(), Intent::NONE);
    }
    assert_eq!(world.enemy_count(), 0, "the condition was shut and something spawned anyway");

    // Teleporting the player is exactly what a scenario cannot do, which is why
    // this half lives here: it isolates the opening of the condition from the
    // ticks spent walking to it.
    world.player.pos = far;
    world.step(tick_dt(), Intent::NONE);

    assert_eq!(world.enemy_count(), 1, "a ready source did not fire the tick its gate opened");
}

/// A population condition refills what was killed and then stops, rather than
/// keeping a promise about a number of emissions into an arena that no longer
/// needs them.
#[test]
fn a_population_condition_fills_to_its_number_and_stops() {
    let mut world = World::default();
    world.set_enemy_count(0);
    world.add_source(
        Source::new(Placement::Ring { centre: Vec2::new(30.0, 0.0), radius: 3.0 }, Template::BODY)
            .when(Condition::FewerThan(3)),
    );

    for _ in 0..60 {
        world.step(tick_dt(), Intent::NONE);
    }
    assert_eq!(world.enemy_count(), 3, "the population condition overshot or stalled");

    // Kill one, and it is replaced — which is the half a fixed emission count
    // cannot do.
    let victim = world.enemies.slots.ids()[0];
    assert!(world.despawn_enemy(victim));
    world.step(tick_dt(), Intent::NONE);
    assert_eq!(world.enemy_count(), 3, "a body was killed and not replaced");
}

/// Removing a source stops the flow, and the name it had never resolves again.
///
/// Ids are not recycled, so this is not the generational hazard `EntityId`
/// exists for — it is the *reason* a source can get away without a generation,
/// and it is worth a test rather than a comment.
#[test]
fn a_removed_source_stops_and_its_name_stays_dead() {
    let mut world = World::default();
    world.set_enemy_count(0);

    let id = world.add_source(Source::new(Placement::At(Vec2::new(30.0, 0.0)), Template::BODY));
    for _ in 0..5 {
        world.step(tick_dt(), Intent::NONE);
    }
    let made = world.enemy_count();
    assert_eq!(made, 5, "every=1 should fire on every tick");

    assert!(world.remove_source(id));
    assert!(!world.remove_source(id), "a dead source was removed twice");
    assert_eq!(world.source_count(), 0);

    for _ in 0..20 {
        world.step(tick_dt(), Intent::NONE);
    }
    assert_eq!(world.enemy_count(), made, "a removed source kept spawning");

    let newcomer = world.add_source(Source::new(Placement::At(Vec2::ZERO), Template::BODY));
    assert_ne!(newcomer, id, "a source id was recycled, which its lack of a generation forbids");
    assert!(!world.remove_source(id), "the dead name resolved to the source that came after it");
}

/// A ring puts successive bodies at different points, which is the whole reason
/// it exists: bodies stacked on one spot are coincident, and the separation
/// solver's response to that is to blow the pile apart.
#[test]
fn a_ring_does_not_stack_what_it_makes() {
    let mut world = World::default();
    world.set_enemy_count(0);
    world.add_source(Source::new(
        Placement::Ring { centre: Vec2::new(30.0, 0.0), radius: 3.0 },
        Template::BODY,
    ));

    // One tick each, and read the positions before the solver has had a chance
    // to separate anything — so this measures the placement, not the response
    // to a bad placement.
    for _ in 0..4 {
        world.step(tick_dt(), Intent::NONE);
    }

    let places = world.enemies.pos.clone();
    assert_eq!(places.len(), 4);
    for (i, a) in places.iter().enumerate() {
        for b in &places[i + 1..] {
            assert!(a.distance(*b) > 2.0 * ENEMY_RADIUS, "a ring stacked {a} on {b}");
        }
    }
}
