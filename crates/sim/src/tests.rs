//! Tests for the world itself: determinism, presentation snapshots, and the budget.
//!
//! Split from `lib.rs` so that file stays the shape it describes — storage, the
//! two seams, and the schedule — rather than being mostly test code.

use super::*;
use arpg_core::MoveDir;

/// Counts only this thread's allocations to isolate parallel tests.
/// Const initialization avoids allocator recursion on first access.
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
    Accumulator::default()
        .pending(Dt::SECS)
        .next()
        .expect("one tick's worth buys one tick")
}

const ENEMY_SPACING: f32 = 0.7;
const _: () = assert!(ENEMY_SPACING > 2.0 * ENEMY_RADIUS);

/// Centred crowd fixture for physical interaction tests.
fn grid_positions(n: usize) -> impl Iterator<Item = Vec2> {
    let side = (n as f32).sqrt().ceil().max(1.0) as usize;
    let offset = (side as f32 - 1.0) * ENEMY_SPACING * 0.5;
    (0..n).map(move |i| {
        Vec2::new(
            (i % side) as f32 * ENEMY_SPACING - offset,
            (i / side) as f32 * ENEMY_SPACING - offset,
        )
    })
}

/// Ordinary construction for tests needing a crowd of a particular size.
fn world_with_enemies(count: usize) -> World {
    let mut world = World::empty();
    assert!(world.set_mass(world.player_id(), Mass::try_from(20.0).unwrap()));
    for position in grid_positions(count) {
        world
            .place(position, crate::TEST_CHARACTER)
            .expect("test crowd fits");
    }
    world
}

/// Test-owned physical drive, independent of gameplay selection and ability state.
fn step(world: &mut World, dt: Dt, direction: MoveDir) {
    let direction = direction.as_vec3();
    let command = MotorCommand::new(
        world.player_id(),
        Vec2::new(direction.x, direction.z) * 9.0,
        60.0,
    )
    .unwrap();
    let population = world.damageable_count();
    world
        .begin_step(dt, population)
        .advance(&[command], &mut ())
        .finish();
}

/// Steps a fresh world `ticks` times and returns the hash after each one.
///
/// The sequence, not the final value: two runs that end up in the same
/// place having taken different routes are still a divergence, and a final
/// -state comparison calls them equal.
fn hash_sequence(enemies: usize, ticks: usize, dir_at: impl Fn(u64) -> MoveDir) -> Vec<u64> {
    let mut world = world_with_enemies(enemies);

    let mut acc = Accumulator::default();
    let mut seq = Vec::with_capacity(ticks);

    while seq.len() < ticks {
        for dt in acc.pending(Dt::SECS) {
            let dir = dir_at(world.tick());
            step(&mut world, dt, dir);
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
    let mut world = world_with_enemies(1);

    // The lone body spawns on top of the player. Two seconds of powered
    // movement puts the player well past it.
    for _ in 0..120 {
        step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
    }
    world
}

#[test]
fn frame_rate_cannot_change_the_simulation() {
    // Powers of two, so every delta is exact in binary floating point: the
    // claim under test is about the accumulator, not about whether a third
    // of a tick rounds. Capped by `MAX_TICKS_PER_FRAME`, so 8 would silently
    // measure the stall path instead.
    let at = |ticks_per_frame: usize| {
        let mut world = world_with_enemies(64);

        let mut acc = Accumulator::default();
        let mut seq = Vec::new();

        for _ in 0..(120 / ticks_per_frame) {
            for dt in acc.pending(Dt::SECS * ticks_per_frame as f32) {
                step(&mut world, dt, MoveDir::new(Vec3::X));
                seq.push(world.hash());
            }
        }
        seq
    };

    let reference = at(1);
    assert_eq!(
        reference.len(),
        120,
        "the reference run did not take the ticks it was given"
    );

    for n in [2, 4] {
        let other = at(n);
        assert_eq!(
            other.len(),
            reference.len(),
            "{n} ticks per frame ran a different number"
        );

        let diverged = reference.iter().zip(&other).position(|(a, b)| a != b);
        assert_eq!(
            diverged, None,
            "{n} ticks per frame diverged at tick {diverged:?}"
        );
    }
}

/// **The sensitivity check, and it is not optional.** A `hash()` that
/// returned a constant would pass the frame-rate test and every replay
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
    assert_eq!(
        straight[..split],
        turning[..split],
        "streams differed before they differed"
    );
    assert_ne!(
        straight[split], turning[split],
        "the first differing tick hashed the same"
    );
    assert_ne!(straight.last(), turning.last(), "the divergence washed out");
}

/// Consume the same allocation-free snapshots that presentation receives.
fn observe_presentation(world: &World, alpha: Alpha) {
    std::hint::black_box(world.player_presentation(alpha));
    for enemy in world.character_presentations(alpha) {
        std::hint::black_box(enemy);
    }
    for prop in world.prop_presentations(alpha) {
        std::hint::black_box(prop);
    }
}

/// Character presentation supplies the interpolated origin; compare body centres.
fn drawn_enemies(world: &World, alpha: Alpha) -> Vec<Vec3> {
    world
        .character_presentations(alpha)
        .map(|enemy| enemy.ground_position() + Vec3::Y * ENEMY_HALF_HEIGHT)
        .collect()
}

/// Puts the player inside the horde and walks, so the solver is displacing
/// bodies every tick. Anything asking whether the *horde* is drawn right
/// has to be measured somewhere the horde actually moves — in open ground
/// every body sits still and a broken blend is indistinguishable from a
/// working one.
fn shoving_through_the_crowd(enemies: usize) -> World {
    let mut world = world_with_enemies(enemies);

    // A short drive keeps the player inside the crowd while contacts move bodies.
    for _ in 0..5 {
        step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
    }
    assert!(
        world.contacts() > 0,
        "nothing is in contact, so nothing is being pushed"
    );
    world
}

/// Reads the player transform through the same snapshot as character rendering.
fn drawn_player(world: &World, alpha: Alpha) -> Vec3 {
    world.player_presentation(alpha).ground_position() + Vec3::Y * PLAYER_HALF_HEIGHT
}

/// **The gate for render interpolation.** The endpoints have to be exact,
/// or the blend is drawing something the simulation never believed.
#[test]
fn the_blend_endpoints_are_the_two_ticks_themselves() {
    let mut world = in_open_ground();

    let before = world.player_pos();
    step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
    let after = world.player_pos();
    assert_ne!(before, after, "the tick under test did not move anything");

    assert_eq!(
        drawn_player(&world, Alpha::ZERO),
        before,
        "alpha 0 is not the previous tick"
    );
    assert_eq!(
        drawn_player(&world, Alpha::ONE),
        after,
        "alpha 1 is not the current tick"
    );
}

/// Asset animation receives one immutable snapshot of simulation-owned facts.

#[test]
fn horde_presentation_is_stable_derived_and_excludes_props() {
    let mut world = World::empty();
    let enemy = world
        .place(Vec2::new(2.0, 0.0), crate::TEST_CHARACTER)
        .unwrap();
    let prop = world.place(Vec2::new(-2.0, 0.0), Template::BLOCK).unwrap();
    step(&mut world, tick_dt(), MoveDir::NONE);

    let presentations: Vec<_> = world.character_presentations(Alpha::ONE).collect();
    assert_eq!(presentations.len(), 1);
    assert_eq!(presentations[0].id(), enemy);
    assert_eq!(presentations[0].ground_position(), Vec3::new(2.0, 0.0, 0.0));
    assert_eq!(presentations[0].displacement(), Vec2::ZERO);
    assert!(world.is_alive(prop));

    let props: Vec<_> = world.prop_presentations(Alpha::ONE).collect();
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].id(), prop);
    assert_eq!(props[0].ground_position(), Vec3::new(-2.0, 0.0, 0.0));
    assert_eq!(props[0].interaction(), None);
}

/// Between the endpoints it has to actually be *between*, and monotonic —
/// a blend that jumps or backtracks is judder wearing a different hat.
#[test]
fn the_blend_crosses_the_gap_once_and_in_order() {
    let mut world = in_open_ground();

    let before = world.player_pos();
    step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
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
        let drawn = drawn_player(&world, alpha);
        let a = alpha.get();

        // On the segment: the two legs sum to the whole only for a point
        // between the ends.
        let off = (drawn - before).length() + (after - drawn).length() - span;
        assert!(
            off.abs() < 1e-4,
            "the drawn position left the segment at alpha {a} by {off}"
        );

        // And moving forward along it, never back.
        let progress = (drawn - before).length();
        assert!(
            progress >= furthest - 1e-6,
            "the blend went backwards at alpha {a}"
        );
        furthest = progress;
    }

    assert!(
        (furthest - span).abs() < 1e-4,
        "the blend reached {furthest}, the tick moved {span}"
    );
}

#[test]
fn the_horde_is_interpolated_too() {
    let mut world = shoving_through_the_crowd(256);

    let before: Vec<Vec2> = world.bodies.pos[1..].to_vec();
    step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
    let after: Vec<Vec2> = world.bodies.pos[1..].to_vec();

    let moved: Vec<usize> = (0..after.len())
        .filter(|&i| before[i] != after[i])
        .collect();
    assert!(
        !moved.is_empty(),
        "no body moved during the tick under test"
    );

    let at_zero = drawn_enemies(&world, Alpha::ZERO);
    let at_one = drawn_enemies(&world, Alpha::ONE);
    let at_half = drawn_enemies(&world, half());

    for i in 0..after.len() {
        assert_eq!(
            at_zero[i],
            on_ground(before[i], ENEMY_HALF_HEIGHT),
            "body {i} at alpha 0"
        );
        assert_eq!(
            at_one[i],
            on_ground(after[i], ENEMY_HALF_HEIGHT),
            "body {i} at alpha 1"
        );
    }

    for &i in &moved {
        let span = (at_one[i] - at_zero[i]).length();
        let off = (at_half[i] - at_zero[i]).length() + (at_one[i] - at_half[i]).length() - span;
        assert!(
            off.abs() < 1e-5,
            "body {i} left the segment between its two ticks"
        );
        assert_ne!(
            at_half[i], at_zero[i],
            "body {i} did not move off its previous tick"
        );
        assert_ne!(at_half[i], at_one[i], "body {i} was drawn already arrived");
    }
}

/// An empty arena has to work, not merely not crash: it is where a fight
/// ends, and it is the only setup in which a scenario can predict plain
/// movement without predicting the solver too.
#[test]
fn an_empty_horde_is_a_legal_world() {
    let mut world = World::empty();
    assert_eq!(world.damageable_count(), 0);

    for _ in 0..30 {
        step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
    }
    assert_eq!(world.contacts(), 0, "an empty arena reported a contact");

    // The bounded motor accelerates by 1m/s per tick after 10% drag, reaching
    // 9m/s on tick 22. Each tick's change (drag included) ramps in over four
    // substeps, so a tick covers (v + 5/8 change) dt; the first 30 ticks cover
    // this distance.
    let expected = 3.457_878_5;
    assert!((world.player_pos().x - expected).abs() < 1e-4);

    assert_eq!(world.character_presentations(Alpha::ONE).count(), 0);
}

/// Newly placed bodies must render standing still before their first tick.
#[test]
fn a_new_horde_is_drawn_standing_still() {
    let world = world_with_enemies(64);

    let standing: Vec<Vec3> = world.bodies.pos[1..]
        .iter()
        .map(|&p| on_ground(p, ENEMY_HALF_HEIGHT))
        .collect();

    for alpha in [Alpha::ZERO, half(), Alpha::ONE] {
        assert_eq!(
            drawn_enemies(&world, alpha),
            standing,
            "a horde that has not been stepped was drawn mid-move at alpha {}",
            alpha.get()
        );
    }
}

/// **The rule that makes interpolation safe**, checked rather than assumed:
/// drawing must not change what the simulation believes. Presentation takes
/// `&self`, so this cannot fail without the signature changing — which is
/// the point, and is why the assertion is cheap enough to keep.
#[test]
fn drawing_never_touches_sim_state() {
    let mut world = world_with_enemies(64);
    for _ in 0..10 {
        step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
    }

    let untouched = world.hash();

    for i in 0..=8 {
        let mut acc = Accumulator::default();
        acc.pending(Dt::SECS * i as f32 / 8.0);
        observe_presentation(&world, acc.alpha());
        assert_eq!(
            world.hash(),
            untouched,
            "presentation at {i}/8 of a tick changed the world"
        );
    }
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
    let mut world = world_with_enemies(16);

    // Out in the open, standing still: placement overlap with the player
    // relaxes at the contact spring's rate until corrections fall below f32
    // resolution, then nothing moves at all.
    for _ in 0..180 {
        step(&mut world, tick_dt(), MoveDir::NONE);
    }
    assert_eq!(
        world.contacts(),
        0,
        "something is touching, so this measures the solver"
    );

    assert_eq!(
        drawn_enemies(&world, half()),
        drawn_enemies(&world, Alpha::ONE),
    );
    assert_eq!(
        drawn_player(&world, half()),
        drawn_player(&world, Alpha::ONE),
    );
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
        let expected = world.bodies.pos[0];
        let enemies_before = world.bodies.pos[1..].to_vec();

        step(&mut world, tick_dt(), east);

        assert_eq!(
            world.bodies.prev_pos[0], expected,
            "the player's prev is not last tick"
        );
        assert_eq!(
            world.bodies.prev_pos[1..],
            enemies_before,
            "the horde's prev is not last tick"
        );
    }
}

/// Replay alone misses consistently omitted state. Changing enemy positions
/// must affect the hash even when player input is unchanged.
#[test]
fn every_field_of_the_world_reaches_the_hash() {
    /// A field of `World` and the smallest change that touches it.
    type Poke = (&'static str, fn(&mut World));

    let fields: [Poke; 9] = [
        ("contact material", |w| {
            w.set_contact_material(ContactMaterial::new(20.0, 10.0, 3.0).unwrap());
        }),
        ("player.pos", |w| w.bodies.pos[0].x += 0.001),
        ("tick", |w| w.tick += 1),
        ("contacts", |w| w.contacts += 1),
        ("enemies.pos", |w| w.bodies.pos[1].x += 0.001),
        ("interactions", |w| {
            w.bodies.interactions.grant(w.bodies.slots.ids()[1])
        }),
        ("enemies.health", |w| {
            w.bodies
                .health
                .hit(w.bodies.slots.ids()[1], core::num::NonZeroU8::MIN);
        }),
        // A pending request is a body that exists in one of two otherwise
        // identical worlds one tick from now.
        ("queue", |w| {
            assert!(w.request_spawn(Vec2::ZERO, crate::TEST_CHARACTER).is_some())
        }),
        // A source's own state decides when and where the next body appears,
        // so two worlds agreeing on every body still diverge from here.
        ("sources", |w| {
            w.add_source(
                Source::try_from(SourceSpec {
                    pos: (0.0, 0.0),
                    radius: 0.0,
                    every: 1,
                    when: Condition::Always,
                    what: crate::TEST_CHARACTER,
                    enabled: true,
                })
                .unwrap(),
            );
        }),
    ];

    for (field, poke) in fields {
        let mut world = world_with_enemies(1024);
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
    let mut world = world_with_enemies(64);

    // Stand still. Only the solver moves anything, so any change in the
    // hash from here is the horde's.
    let settled = {
        for _ in 0..30 {
            step(&mut world, tick_dt(), MoveDir::NONE);
        }
        world.hash()
    };

    world.bodies.pos[1..][7] += Vec2::new(0.01, -0.01);
    assert_ne!(
        world.hash(),
        settled,
        "displacing a body left the hash unchanged"
    );
}

/// Walking into the wall must stop, not leave the ground plane — and must
/// stay finite, since a NaN position would silently vanish the character.
#[test]
fn the_player_cannot_walk_off_the_arena() {
    let mut world = world_with_enemies(1024);
    for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
        // Twenty seconds at `PLAYER_SPEED` is 180 units — comfortably past
        // the far wall from anywhere in a 96-unit half-arena, so this
        // reaches the clamp rather than merely walking toward it.
        for _ in 0..1200 {
            step(&mut world, tick_dt(), MoveDir::new(dir));
        }
        let pos = world.player_pos();
        assert!(pos.is_finite());
        assert!(
            pos.x.abs() <= ARENA_HALF && pos.z.abs() <= ARENA_HALF,
            "escaped: {pos}"
        );
    }
}

/// Decoupling rendering must preserve physical admission, through ordinary placement.
#[test]
fn the_physical_population_ceiling_is_preserved() {
    let mut world = World::empty();
    for _ in 0..MAX_BODIES {
        world.place(Vec2::ZERO, crate::TEST_CHARACTER).unwrap();
    }
    assert_eq!(world.damageable_count(), 183_494);
    assert_eq!(world.body_count(), MAX_BODIES);
    assert!(world.place(Vec2::ZERO, crate::TEST_CHARACTER).is_none());
}

/// The count is derived from the storage, so asking for N must actually
/// produce N bodies — not N draw calls over a formula.
#[test]
fn the_horde_holds_exactly_the_requested_count() {
    for n in [1, 17, 512, 1024, 4096] {
        let world = world_with_enemies(n);
        assert_eq!(world.damageable_count(), n);
        assert_eq!(world.bodies.pos[1..].len(), n);
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
    let world = world_with_enemies(1024);

    let pos = &world.bodies.pos[1..];
    let mut closest = f32::MAX;
    for i in 0..pos.len() {
        for j in i + 1..pos.len() {
            closest = closest.min(pos[i].distance(pos[j]));
        }
    }

    assert!(
        closest > (2.0 * ENEMY_RADIUS),
        "spawned {closest} apart, but a body is {} wide",
        (2.0 * ENEMY_RADIUS)
    );
}

/// Non-square populations have a partial last row; allow one spacing of centroid offset.
#[test]
fn the_horde_is_centred_on_the_origin() {
    let centroid_at = |world: &World| {
        let pos = &world.bodies.pos[1..];
        pos.iter().fold(Vec2::ZERO, |acc, &p| acc + p) / pos.len() as f32
    };

    for n in [1, 4, 1024] {
        let world = world_with_enemies(n);
        let c = centroid_at(&world);
        assert!(
            c.length() < 1e-3,
            "square N={n} should be exactly centred, got {c}"
        );
    }

    for n in [17, 500, 4095] {
        let world = world_with_enemies(n);
        let c = centroid_at(&world);
        assert!(
            c.length() < ENEMY_SPACING,
            "ragged N={n} drifted {c}, more than one row"
        );
    }
}

/// The one place `Vec2::y` means world Z, so it is worth pinning: a
/// transposition here is horizontal either way and would draw the whole
/// horde mirrored along a diagonal without a single test failing elsewhere.
#[test]
fn a_ground_position_keeps_x_and_lifts_y_into_z() {
    assert_eq!(
        on_ground(Vec2::new(3.0, -7.0), 0.25),
        Vec3::new(3.0, 0.25, -7.0)
    );
}

/// **The invariant the pass exists to establish**, checked on the real
/// world rather than on a pair: after a step, nothing is inside the player.
#[test]
fn no_enemy_is_left_overlapping_the_player() {
    let mut world = world_with_enemies(1024);

    // Walk into the middle of the horde and keep going.
    for _ in 0..240 {
        step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
    }

    let contact = PLAYER_RADIUS + ENEMY_RADIUS;
    let player = world.bodies.pos[0];
    for (i, &enemy) in world.bodies.pos[1..].iter().enumerate() {
        let gap = player.distance(enemy);
        assert!(
            gap >= contact - 1e-4,
            "enemy {i} is {gap} from the player, needs {contact}"
        );
    }
}

/// Walking through the horde must displace it. A player that leaves the
/// crowd exactly as it found it is not colliding with anything, which is a
/// failure the previous test cannot see — it passes trivially if nothing
/// ever overlaps because nothing ever touches.
#[test]
fn walking_through_the_horde_displaces_it() {
    let mut world = world_with_enemies(1024);
    let before = world.bodies.pos[1..].to_vec();

    let mut ever_touched = 0;
    for _ in 0..240 {
        step(&mut world, tick_dt(), MoveDir::new(Vec3::X));
        ever_touched += world.contacts();
    }

    let moved = before
        .iter()
        .zip(&world.bodies.pos[1..])
        .filter(|(a, b)| a.distance(**b) > 1e-4)
        .count();

    assert!(ever_touched > 0, "nothing was ever in contact");
    assert!(
        moved > 0,
        "the player walked straight through {} bodies",
        before.len()
    );
}

/// The contact count has to mean something, or it is a comforting number
/// that would keep reporting zero if the solver stopped working. Standing
/// clear of everything is zero; standing inside the horde is not.
#[test]
fn the_contact_count_tracks_whether_anything_is_touching() {
    let mut clear = in_open_ground();
    step(&mut clear, tick_dt(), MoveDir::NONE);
    assert_eq!(clear.contacts(), 0, "nothing is near the player out here");

    // Structural overlap is repaired without an impact. Subsequent physical
    // pushing must report the player's compressed contacts.
    let mut crowded = world_with_enemies(1024);
    step(&mut crowded, tick_dt(), MoveDir::NONE);
    let mut touched = false;
    for _ in 0..30 {
        step(&mut crowded, tick_dt(), MoveDir::new(Vec3::X));
        touched |= crowded.contacts() > 0;
    }
    assert!(touched, "physical pushing reported no contact");
}

/// Everything stays inside the world, including bodies that only moved
/// because something shoved them.
#[test]
fn nothing_is_pushed_out_of_the_arena() {
    let mut world = world_with_enemies(256);

    for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
        for _ in 0..600 {
            step(&mut world, tick_dt(), MoveDir::new(dir));
        }
        for &enemy in &world.bodies.pos[1..] {
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
    let mut world = World::empty();

    assert!(
        world
            .request_spawn(Vec2::new(3.0, 0.0), crate::TEST_CHARACTER)
            .is_some()
    );
    assert_eq!(
        world.damageable_count(),
        0,
        "the request was granted without a tick running"
    );

    step(&mut world, tick_dt(), MoveDir::NONE);
    assert_eq!(world.damageable_count(), 1);
}

/// **A dropped spawn must not be silent.** Nothing appears, nothing errors, and
/// the only other evidence would be a body somebody expected and did not get —
/// so the refusal is a return value at the door and an event in the trace.
#[test]
fn a_full_queue_refuses_out_loud() {
    use crate::pass::spawn::QUEUE_CAPACITY;

    let mut world = World::empty();
    world.clear_trace();

    let tickets: Vec<_> = (0..QUEUE_CAPACITY)
        .map(|i| {
            world
                .request_spawn(Vec2::ZERO, crate::TEST_CHARACTER)
                .unwrap_or_else(|| panic!("request {i} refused early"))
        })
        .collect();
    assert!(
        world
            .request_spawn(Vec2::ZERO, crate::TEST_CHARACTER)
            .is_none(),
        "the queue accepted past capacity"
    );

    step(&mut world, tick_dt(), MoveDir::NONE);

    assert_eq!(
        world.damageable_count(),
        QUEUE_CAPACITY,
        "the accepted requests were not all granted"
    );
    assert_eq!(
        world
            .spawn_outcomes()
            .iter()
            .map(|result| result.ticket)
            .collect::<Vec<_>>(),
        tickets
    );
    assert!(
        world
            .spawn_outcomes()
            .iter()
            .all(|result| result.entity.is_some())
    );
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "this assertion counts refusals; unrelated trace event kinds do not participate"
    )]
    let refusals: Vec<_> = world
        .trace()
        .iter()
        .filter_map(|(_, e)| match e {
            Event::Refused { count } => Some(count),
            _ => None,
        })
        .collect();
    assert_eq!(
        refusals,
        vec![1],
        "the dropped request left no trace of itself"
    );
}

#[test]
fn a_held_back_source_fires_the_instant_its_condition_opens() {
    let mut world = World::empty();

    let far = Vec2::new(30.0, 0.0);
    world.add_source(
        Source::try_from(SourceSpec {
            pos: far.into(),
            radius: 0.0,
            every: 60,
            when: Condition::PlayerWithin(5.0),
            what: crate::TEST_CHARACTER,
            enabled: true,
        })
        .unwrap(),
    );

    // Long enough that a source counting down regardless would have come round
    // twice over.
    for _ in 0..120 {
        step(&mut world, tick_dt(), MoveDir::NONE);
    }
    assert_eq!(
        world.damageable_count(),
        0,
        "the condition was shut and something spawned anyway"
    );

    // Teleporting the player is exactly what a scenario cannot do, which is why
    // this half lives here: it isolates the opening of the condition from the
    // ticks spent walking to it.
    world.bodies.pos[0] = far;
    step(&mut world, tick_dt(), MoveDir::NONE);

    assert_eq!(
        world.damageable_count(),
        1,
        "a ready source did not fire the tick its gate opened"
    );
}

/// A population condition refills what was killed and then stops, rather than
/// keeping a promise about a number of emissions into an arena that no longer
/// needs them.
#[test]
fn a_population_condition_fills_to_its_number_and_stops() {
    let mut world = World::empty();
    world.add_source(
        Source::try_from(SourceSpec {
            pos: (30.0, 0.0),
            radius: 3.0,
            every: 1,
            when: Condition::FewerThan(3),
            what: crate::TEST_CHARACTER,
            enabled: true,
        })
        .unwrap(),
    );

    for _ in 0..60 {
        step(&mut world, tick_dt(), MoveDir::NONE);
    }
    assert_eq!(
        world.damageable_count(),
        3,
        "the population condition overshot or stalled"
    );

    // Kill one, and it is replaced — which is the half a fixed emission count
    // cannot do.
    let victim = world.bodies.slots.ids()[1];
    assert!(world.despawn_body(victim));
    step(&mut world, tick_dt(), MoveDir::NONE);
    assert_eq!(
        world.damageable_count(),
        3,
        "a body was killed and not replaced"
    );
}

/// Removing a source stops the flow, and the name it had never resolves again.
///
/// Ids are not recycled, so this is not the generational hazard `EntityId`
/// exists for — it is the *reason* a source can get away without a generation,
/// and it is worth a test rather than a comment.
#[test]
fn a_removed_source_stops_and_its_name_stays_dead() {
    let mut world = World::empty();

    let id = world.add_source(
        Source::try_from(SourceSpec {
            pos: (30.0, 0.0),
            radius: 0.0,
            every: 1,
            when: Condition::Always,
            what: crate::TEST_CHARACTER,
            enabled: true,
        })
        .unwrap(),
    );
    for _ in 0..5 {
        step(&mut world, tick_dt(), MoveDir::NONE);
    }
    let made = world.damageable_count();
    assert_eq!(made, 5, "every=1 should fire on every tick");

    assert!(world.remove_source(id));
    assert!(!world.remove_source(id), "a dead source was removed twice");
    assert_eq!(world.source_count(), 0);

    for _ in 0..20 {
        step(&mut world, tick_dt(), MoveDir::NONE);
    }
    assert_eq!(
        world.damageable_count(),
        made,
        "a removed source kept spawning"
    );

    let newcomer = world.add_source(
        Source::try_from(SourceSpec {
            pos: (0.0, 0.0),
            radius: 0.0,
            every: 1,
            when: Condition::Always,
            what: crate::TEST_CHARACTER,
            enabled: true,
        })
        .unwrap(),
    );
    assert_ne!(
        newcomer, id,
        "a source id was recycled, which its lack of a generation forbids"
    );
    assert!(
        !world.remove_source(id),
        "the dead name resolved to the source that came after it"
    );
}

/// A ring puts successive bodies at different points, which is the whole reason
/// it exists: bodies stacked on one spot are coincident, and the separation
/// solver's response to that is to blow the pile apart.
#[test]
fn a_ring_does_not_stack_what_it_makes() {
    let mut world = World::empty();
    world.add_source(
        Source::try_from(SourceSpec {
            pos: (30.0, 0.0),
            radius: 3.0,
            every: 1,
            when: Condition::Always,
            what: crate::TEST_CHARACTER,
            enabled: true,
        })
        .unwrap(),
    );

    // One tick each, and read the positions before the solver has had a chance
    // to separate anything — so this measures the placement, not the response
    // to a bad placement.
    for _ in 0..4 {
        step(&mut world, tick_dt(), MoveDir::NONE);
    }

    let places = world.bodies.pos[1..].to_vec();
    assert_eq!(places.len(), 4);
    for (i, a) in places.iter().enumerate() {
        for b in &places[i + 1..] {
            assert!(
                a.distance(*b) > 2.0 * ENEMY_RADIUS,
                "a ring stacked {a} on {b}"
            );
        }
    }
}

/// Physical and health payloads follow identity through their row changes.
#[test]
fn despawning_preserves_a_swapped_survivors_payloads() {
    let mut world = World::empty();
    let a = world
        .place(Vec2::new(10.0, 0.0), crate::TEST_CHARACTER)
        .unwrap();
    let b = world
        .place(Vec2::new(20.0, 0.0), crate::TEST_CHARACTER)
        .unwrap();
    let impulse = Impulse::try_from((6.0, 0.0)).unwrap();
    assert!(world.apply_impulse(a, impulse));
    assert!(world.apply_impulse(b, impulse));
    assert_eq!(
        world.bodies.health.hit(b, core::num::NonZeroU8::MIN),
        Some(2)
    );
    assert!(world.despawn_body(a));
    let c = world
        .place(Vec2::new(30.0, 0.0), crate::TEST_CHARACTER)
        .unwrap();
    assert!(!world.apply_impulse(a, impulse));
    assert!(world.motion(a).is_none());
    assert_eq!(world.motion(c).unwrap().velocity(), Vec2::ZERO);
    assert_eq!(world.motion(b).unwrap().velocity(), Vec2::new(6.0, 0.0));
    assert_eq!(world.health(b), Some(2));
    assert_eq!(world.health(c), Some(3));
    step(&mut world, tick_dt(), MoveDir::NONE);
    assert!((world.body_pos(b).unwrap().x - 20.1).abs() < 1e-5);
    assert_eq!(world.body_pos(c).unwrap().x, 30.0);
}

#[test]
fn velocity_is_hashed_observed_and_does_not_change_zero_tick_rendering() {
    let mut world = World::empty();
    let id = world
        .place(Vec2::new(10.0, 0.0), crate::TEST_CHARACTER)
        .unwrap();
    let before = world.hash();
    let position = world.body_pos(id);
    assert!(world.apply_impulse(id, Impulse::try_from((6.0, 0.0)).unwrap()));
    assert_ne!(world.hash(), before);
    assert_eq!(world.body_pos(id), position);
    for alpha in [Alpha::ZERO, half(), Alpha::ONE] {
        assert_eq!(drawn_enemies(&world, alpha), vec![position.unwrap()]);
    }
    let mut report = Report::default();
    world.report(&mut report);
    let report = report.finish();
    assert!(
        report.contains("\"velocity\":[6.0000,0.0000,0.0000]"),
        "{report}"
    );
}

/// Eviction revokes prop membership, including before the slot is recycled.
#[test]
fn scene_eviction_revokes_prop_activation() {
    let scene = Scene {
        name: "prop".into(),
        bodies: vec![Placed {
            pos: (0.0, 1.0),
            what: Template::BLOCK.interactive(),
        }],
        ..Scene::default()
    };
    let mut world = World::empty();
    let owner = world.load_scene(&scene).unwrap();
    let id = world.bodies.slots.ids()[1];
    assert!(!world.actor_view().movable(id));
    let player = world.player_id();
    let population = world.damageable_count();
    let mut resolved = world
        .begin_step(tick_dt(), population)
        .advance(&[], &mut ());
    resolved.interact_nearest(player);
    resolved.finish();

    assert_eq!(
        world.interaction_state(id),
        Some(InteractionState::Activated)
    );
    assert_eq!(world.body_count(), 1);
    assert!(world.evict_scene(owner));
    assert!(!world.is_alive(id));
    assert_eq!(world.interaction_state(id), None);
    let replacement = world.place(Vec2::Y, Template::BLOCK.interactive()).unwrap();
    assert_eq!(id.slot(), replacement.slot());
    assert_eq!(
        world.interaction_state(replacement),
        Some(InteractionState::Ready)
    );
    assert_eq!(world.interaction_state(id), None);
    let fresh = World::from_scene(&scene).unwrap();
    assert_eq!(
        fresh.interaction_state(fresh.bodies.slots.ids()[1]),
        Some(InteractionState::Ready)
    );
}

/// A spawned prop consumes the same instance budget as an enemy, even though
/// it does not contribute to enemy counts or source conditions.
#[test]
fn props_share_capacity_and_scene_admission_is_atomic() {
    let mut world = World::empty();
    let prop = world.place(Vec2::Y, Template::BLOCK).unwrap();
    for _ in 0..MAX_BODIES - 1 {
        world.place(Vec2::ZERO, crate::TEST_CHARACTER).unwrap();
    }
    assert_eq!(world.body_count(), MAX_BODIES);
    assert_eq!(world.damageable_count(), MAX_BODIES - 1);
    assert!(world.is_alive(prop));
    let before = world.hash();
    let scene = Scene {
        name: "overflow".into(),
        bodies: vec![Placed {
            pos: (0.0, 2.0),
            what: Template::BLOCK,
        }],
        ..Scene::default()
    };
    assert_eq!(world.load_scene(&scene), Err(SceneError::Capacity));
    assert_eq!(world.hash(), before);
    assert!(world.place(Vec2::Y, Template::BLOCK).is_none());
}

/// Selection does not depend on the member's dense row after a removal swap.
#[test]
fn an_interaction_tie_survives_dense_row_swaps() {
    let mut world = World::empty();
    let removed = world
        .place(Vec2::new(5.0, 0.0), Template::BLOCK.interactive())
        .unwrap();
    let first = world.place(Vec2::Y, Template::BLOCK.interactive()).unwrap();
    let last = world
        .place(-Vec2::Y, Template::BLOCK.interactive())
        .unwrap();
    world.despawn_body(removed);
    let player = world.player_id();
    let population = world.damageable_count();
    let mut resolved = world
        .begin_step(tick_dt(), population)
        .advance(&[], &mut ());
    resolved.interact_nearest(player);
    resolved.finish();
    assert_eq!(
        world.interaction_state(first),
        Some(InteractionState::Activated)
    );
    assert_eq!(world.interaction_state(last), Some(InteractionState::Ready));
}

/// Replay agreement alone cannot detect a missing field in the hash. These
/// worlds differ only in activation after their otherwise identical tick.
#[test]
fn activation_state_reaches_the_world_hash() {
    let make = || {
        let mut world = World::empty();
        world.place(Vec2::Y, Template::BLOCK.interactive()).unwrap();
        world
    };
    let (mut ready, mut activated) = (make(), make());
    step(&mut ready, tick_dt(), MoveDir::NONE);
    let player = activated.player_id();
    let mut resolved = activated.begin_step(tick_dt(), 0).advance(&[], &mut ());
    resolved.interact_nearest(player);
    resolved.finish();
    assert_ne!(ready.hash(), activated.hash());
}

#[test]
fn crowd_contacts_replay_through_combat_and_structural_changes() {
    for seed in 0..8u32 {
        let mut random = seed + 1;
        let mut coordinate = || {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (random % 1201) as f32 * 0.01 - 6.0
        };
        let mut scene = Scene {
            walls: Vec::new(),
            joints: Vec::new(),
            name: "grid oracle".into(),
            bodies: vec![],
            grids: vec![],
            sources: vec![],
        };
        for i in 0..128 {
            let pos = match seed % 4 {
                0 => (coordinate(), coordinate()),
                1 => (2.0, 2.0), // Pathological coincident pile.
                2 => (96.0 + coordinate(), -96.0 + coordinate()), // Before containment.
                _ => ((i % 16) as f32 * 0.49 - 4.0, (i / 16) as f32 * 0.49),
            };
            let what = if i % 13 == 0 && seed % 4 != 2 {
                Template::BLOCK
            } else {
                crate::TEST_CHARACTER
            };
            scene.bodies.push(Placed { pos, what });
        }
        let mut grid = World::from_scene(&scene).unwrap();
        let mut replay = World::from_scene(&scene).unwrap();
        let mut extra = None;
        for tick in 0..180 {
            for world in [&mut grid, &mut replay] {
                if tick % 11 == 0 {
                    let id = world.bodies.slots.ids()[1 + tick % world.body_count()];
                    assert!(world.apply_impulse(id, Impulse::try_from((12.0, -8.0)).unwrap()));
                }
                if tick % 17 == 0 {
                    assert!(
                        world
                            .request_spawn(Vec2::new(0.49, 0.49), crate::TEST_CHARACTER)
                            .is_some()
                    );
                }
                if tick == 25 {
                    let id = world.bodies.slots.ids()[10];
                    assert!(world.despawn_body(id));
                    world
                        .place(Vec2::new(-0.001, 0.0), crate::TEST_CHARACTER)
                        .unwrap();
                    assert!(!world.apply_impulse(id, Impulse::try_from((1.0, 0.0)).unwrap()));
                    assert!(!world.despawn_body(id));
                }
                if tick == 40 {
                    let id = world
                        .load_scene(&Scene {
                            walls: Vec::new(),
                            joints: Vec::new(),
                            name: "temporary".into(),
                            bodies: vec![
                                Placed {
                                    pos: (0.49, 0.0),
                                    what: crate::TEST_CHARACTER,
                                },
                                Placed {
                                    pos: (0.51, 0.0),
                                    what: crate::TEST_CHARACTER,
                                },
                            ],
                            grids: vec![],
                            sources: vec![],
                        })
                        .unwrap();
                    if let Some(previous) = extra {
                        assert_eq!(id, previous);
                    }
                    extra = Some(id);
                }
                if tick == 60 {
                    assert!(world.evict_scene(extra.unwrap()));
                    assert!(!world.evict_scene(extra.unwrap()));
                }
                let before = world.hash();
                assert!(
                    world
                        .place(Vec2::new(f32::NAN, 0.0), crate::TEST_CHARACTER)
                        .is_none()
                );
                assert_eq!(
                    world.hash(),
                    before,
                    "rejected input preserves queued spawns"
                );
                let direction = [Vec3::X, Vec3::Z, -Vec3::X, -Vec3::Z][tick / 30 % 4];
                let population = world.damageable_count();
                let targets = world.bodies.health.ids()[1..].to_vec();
                let player_id = world.player_id();
                let prepared = world.begin_step(tick_dt(), population);
                let view = prepared.view();
                let player = view.position(view.player_id()).unwrap();
                let mut motors: Vec<_> = targets
                    .iter()
                    .copied()
                    .filter(|id| id.slot() % 2 == 0)
                    .map(|id| {
                        let toward =
                            (player - view.position(id).unwrap()).normalize_or_zero() * 2.5;
                        crate::MotorCommand::new(id, toward, 30.0).unwrap()
                    })
                    .collect();
                motors.push(
                    MotorCommand::new(player_id, Vec2::new(direction.x, direction.z) * 9.0, 60.0)
                        .unwrap(),
                );
                let mut resolved = prepared.advance(&motors, &mut ());
                if tick % 23 == 0 {
                    let target = targets[0];
                    resolved.damage(player_id, target, core::num::NonZeroU8::MIN);
                }
                resolved.finish();
            }
            assert_eq!(
                grid.hash(),
                replay.hash(),
                "seed {seed}, tick {tick}: complete physical state"
            );
            assert_eq!(
                grid.trace().render(),
                replay.trace().render(),
                "seed {seed}, tick {tick}: transitions"
            );
            for &position in &grid.bodies.pos {
                assert!(position.abs().max_element() <= crate::ARENA_HALF);
            }
        }
    }
}

#[test]
fn rebuilding_the_grid_uses_reserved_storage_even_after_population_changes() {
    let mut world = world_with_enemies(1024);
    let allocations = alloc_counter::allocations(|| {
        for _ in 0..3 {
            step(&mut world, tick_dt(), MoveDir::NONE);
        }
    });
    assert_eq!(
        allocations, 0,
        "the grid must not allocate, even on its first step"
    );
    let id = world.bodies.slots.ids()[10];
    world.despawn_body(id);
    assert!(
        world
            .request_spawn(Vec2::new(0.49, 0.49), crate::TEST_CHARACTER)
            .is_some()
    );
    assert_eq!(
        alloc_counter::allocations(|| step(&mut world, tick_dt(), MoveDir::NONE)),
        0
    );
}
