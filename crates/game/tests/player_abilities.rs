//! Player ability and presentation invariants belong at the gameplay boundary.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{ActorDefinition, AttackProfile, Game, Tuning, TuningField};
use arpg_sim::{Accumulator, Alpha, AttackPhase, Dt};
use glam::{Vec2, Vec3};

mod alloc_counter {
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

#[global_allocator]
static COUNTING: alloc_counter::Counting = alloc_counter::Counting;

fn tick_dt() -> Dt {
    Accumulator::default().pending(Dt::SECS).next().unwrap()
}
fn half() -> Alpha {
    let mut accumulator = Accumulator::default();
    accumulator.pending(Dt::SECS / 2.0);
    accumulator.alpha()
}
fn world_with_enemies(count: usize) -> Game {
    let mut game = Game::empty();
    let side = (count as f32).sqrt().ceil().max(1.0) as usize;
    let offset = (side as f32 - 1.0) * 0.7 * 0.5;
    for i in 0..count {
        game.place(
            Vec2::new(
                (i % side) as f32 * 0.7 - offset,
                (i / side) as f32 * 0.7 - offset,
            ),
            ActorDefinition::passive_hostile(),
        )
        .unwrap();
    }
    game
}
fn observe_presentation(game: &Game, alpha: Alpha) {
    std::hint::black_box(game.player_presentation(alpha));
    for character in game.character_presentations(alpha) {
        std::hint::black_box(character);
    }
    for prop in game.prop_presentations(alpha) {
        std::hint::black_box(prop);
    }
}

#[test]
fn player_presentation_carries_identity_transform_movement_and_attack() {
    let mut world = Game::empty();
    let id = world.player_id();
    assert!(
        !world.despawn_body(id),
        "the player cannot be removed as an enemy"
    );
    world.step(
        tick_dt(),
        Intent::new(MoveDir::new(Vec3::X), AimDir::new(Vec3::X).unwrap(), true),
    );

    let player = world.player_presentation(Alpha::ONE);
    assert_eq!(player.id(), id);
    assert_eq!(player.ground_position().y, 0.0);
    // The motor's velocity change ramps in over four substeps, so the first
    // tick covers (4 + 1) / (2 * 4) of a * dt².
    let travel =
        Tuning::default().get(TuningField::PlayerAcceleration) * Dt::SECS * Dt::SECS * 5.0 / 8.0;
    assert!((player.ground_position().x - travel).abs() < 1.0e-6);
    assert!((player.displacement() - Vec2::X * travel).length() < 1.0e-6);
    assert_eq!(player.facing(), world.player_facing());
    assert_eq!(player.attack().phase, AttackPhase::Startup);
    assert_eq!(
        world.player_id(),
        id,
        "presentation changed stable identity"
    );
}

#[test]
fn every_configured_and_committed_attack_profile_reaches_the_hash() {
    for profile in AttackProfile::ALL.into_iter().skip(1) {
        let mut configured = world_with_enemies(1024);
        configured.set_attack_profile(profile);
        assert_ne!(
            configured.hash(),
            world_with_enemies(1024).hash(),
            "configured {profile} omitted from replay state"
        );

        let mut changed = Game::empty();
        let mut normal = Game::empty();
        changed.set_attack_profile(profile);
        let press = Intent::new(MoveDir::NONE, AimDir::FORWARD, true);
        changed.step(tick_dt(), press);
        normal.step(tick_dt(), press);
        // Configured profiles agree again; only the in-flight copy differs.
        // Trace is deliberately excluded from the hash.
        changed.set_attack_profile(AttackProfile::default());
        assert_ne!(
            changed.hash(),
            normal.hash(),
            "committed {profile} omitted from replay state"
        );
    }
}

/// Warm up before counting so initial storage preparation is excluded.
#[test]
fn a_steady_state_frame_allocates_nothing() {
    let east = MoveDir::new(Vec3::X);
    let dt = tick_dt();

    for profile in AttackProfile::ALL {
        let mut world = world_with_enemies(512);
        world.set_attack_profile(profile);
        let attack = profile.resolve(Tuning::default());
        let duration = attack.map_or(60, |attack| {
            attack.startup() + attack.active() + attack.recovery().get()
        });

        world.step(
            dt,
            Intent::new(
                east,
                AimDir::new(east.as_vec3()).expect("nonzero test movement"),
                false,
            ),
        );
        observe_presentation(&world, Alpha::ONE);

        let allocations = alloc_counter::allocations(|| {
            for tick in 0..3 * duration {
                // Include three consecutive swings: walking alone cannot catch
                // a hit list that grows when the attack reaches a crowded rank.
                world.step(
                    dt,
                    Intent::new(
                        east,
                        AimDir::new(east.as_vec3()).expect("nonzero test movement"),
                        tick % duration == 0,
                    )
                    .with_attack_held(profile == AttackProfile::Disintegrate),
                );
                observe_presentation(&world, Alpha::ONE);
            }
        });

        assert_eq!(
            allocations, 0,
            "{profile}: three swings allocated {allocations} times"
        );
    }
}

#[test]
fn turning_never_overshoots_its_target() {
    let mut world = world_with_enemies(1024);
    let target = std::f32::consts::FRAC_PI_2;

    for _ in 0..200 {
        world.step(
            tick_dt(),
            Intent::new(MoveDir::new(Vec3::X), AimDir::new(Vec3::X).unwrap(), false),
        );
        assert!(world.player_facing() >= 0.0);
        assert!(
            world.player_facing() <= target,
            "overshot to {}",
            world.player_facing()
        );
    }
}

#[test]
fn facing_stays_wrapped_while_spinning() {
    let mut world = world_with_enemies(1024);
    let circle = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z];

    for lap in 0..50 {
        for _ in 0..30 {
            world.step(
                tick_dt(),
                Intent::new(
                    MoveDir::new(circle[lap % 4]),
                    AimDir::new(circle[lap % 4]).unwrap(),
                    false,
                ),
            );
        }
        assert!(
            world.player_facing().abs() <= std::f32::consts::PI + 1e-6,
            "drifted to {}",
            world.player_facing()
        );
    }
}

#[test]
fn drawn_facing_crosses_the_pi_seam_without_a_full_turn() {
    let mut game = Game::empty();
    let pi = core::f32::consts::PI;
    let intent = |angle: f32| {
        Intent::new(
            MoveDir::NONE,
            AimDir::new(Vec3::new(angle.sin(), 0.0, angle.cos())).unwrap(),
            false,
        )
    };
    for _ in 0..180 {
        game.step(tick_dt(), intent(pi - 0.01));
    }
    let before = game.player_facing();
    game.step(tick_dt(), intent(-pi + 0.01));
    let after = game.player_facing();
    let middle = game.player_presentation(half()).facing();
    assert!(
        middle.abs() > pi - 0.02,
        "interpolation crossed the long arc: {middle}"
    );
    assert!((game.player_presentation(Alpha::ZERO).facing() - before).abs() < 1e-6);
    assert!((game.player_presentation(Alpha::ONE).facing() - after).abs() < 1e-6);
    assert!(middle.abs() <= pi);
}

#[test]
fn facing_reaches_the_game_hash_independently_of_physical_movement() {
    let (mut facing_forward, mut facing_east) = (Game::empty(), Game::empty());
    facing_forward.step(
        tick_dt(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
    facing_east.step(
        tick_dt(),
        Intent::new(MoveDir::NONE, AimDir::new(Vec3::X).unwrap(), false),
    );
    assert_eq!(facing_forward.player_pos(), facing_east.player_pos());
    assert_ne!(facing_forward.player_facing(), facing_east.player_facing());
    assert_ne!(facing_forward.hash(), facing_east.hash());
}

#[test]
fn player_attacks_apply_damage_to_hostiles_and_preserve_nonhostile_health() {
    use arpg_game::ActorRole;
    use arpg_sim::{Event, Template};
    use std::num::NonZeroU8;
    let mut game = Game::empty();
    game.set_attack_profile(AttackProfile::Disintegrate);
    let target = |role| ActorDefinition {
        physical: Template::BLOCK.with_health(NonZeroU8::new(3).unwrap()),
        role,
        ..ActorDefinition::default()
    };
    let eligible = game
        .place(Vec2::new(0.0, 2.0), target(ActorRole::Hostile))
        .unwrap();
    let excluded = game
        .place(Vec2::new(0.0, 3.0), target(ActorRole::Civilian))
        .unwrap();
    game.step(
        tick_dt(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true),
    );
    assert_eq!(game.health(eligible), Some(2));
    assert_eq!(game.health(excluded), Some(3));
    let player = game.player_id();
    assert!(game.trace().iter().any(|(_, event)| event
        == Event::Damaged {
            actor: player,
            target: eligible,
            remaining: 2
        }));
}

#[test]
fn zero_health_player_can_still_begin_and_resolve_an_ability() {
    use arpg_game::{ActionDefinition, ActionTiming, BehaviorNode, SwipeAction};
    use std::num::NonZeroU8;
    let mut game = Game::empty();
    let attacker = game
        .place(
            Vec2::Y,
            ActorDefinition {
                actions: vec![ActionDefinition::Swipe(SwipeAction {
                    timing: ActionTiming {
                        startup: 1,
                        active: 1,
                        recovery: 1,
                    },
                    damage: NonZeroU8::new(5).unwrap(),
                    impulse: 0.0,
                    ..Default::default()
                })],
                behavior: BehaviorNode::Action(0),
                ..ActorDefinition::passive_hostile()
            },
        )
        .unwrap();
    let idle = Intent::new(MoveDir::NONE, AimDir::FORWARD, false);
    game.step(tick_dt(), idle);
    assert_eq!(game.player_health(), 5);
    game.step(tick_dt(), idle);
    assert_eq!(game.player_health(), 0);
    assert!(game.despawn_body(attacker));
    let target = game
        .place(Vec2::new(0.0, 2.0), ActorDefinition::passive_hostile())
        .unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    game.step(tick_dt(), idle.with_attack_held(true));
    assert!(game.beam().is_some());
    assert_eq!(game.health(target), Some(2));
    assert_eq!(game.player_health(), 0);
}
