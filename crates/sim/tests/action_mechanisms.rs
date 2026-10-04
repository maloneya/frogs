//! Physical action boundaries independent of gameplay roles and selection policy.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_sim::{
    Accumulator, Alpha, Condition, Dt, Event, MotorCommand, Source, SourceSpec, Template, World,
};
use glam::Vec2;
use std::num::NonZeroU8;

fn dt() -> Dt {
    Accumulator::default().pending(Dt::SECS).next().unwrap()
}
fn idle() -> Intent {
    Intent::new(MoveDir::NONE, AimDir::FORWARD, false)
}
fn hp(amount: u8) -> NonZeroU8 {
    NonZeroU8::new(amount).unwrap()
}

#[test]
fn neutral_health_and_character_capabilities_are_independent() {
    let mut world = World::empty();
    let neutral = world.place(Vec2::new(10.0, 0.0), Template::BODY).unwrap();
    let character = world
        .place(Vec2::new(20.0, 0.0), Template::BODY.character())
        .unwrap();
    let breakable = world
        .place(Vec2::new(30.0, 0.0), Template::BLOCK.with_health(hp(7)))
        .unwrap();
    assert_eq!(world.health(neutral), None);
    assert_eq!(world.health(character), None);
    assert_eq!(world.health(breakable), Some(7));
    assert_eq!(world.max_health(breakable), Some(7));
    assert_eq!(world.health(world.player_id()), Some(5));
    assert_eq!(
        world
            .character_presentations(Alpha::ONE)
            .map(|p| p.id())
            .collect::<Vec<_>>(),
        [character]
    );
    assert_eq!(
        world
            .prop_presentations(Alpha::ONE)
            .map(|p| p.id())
            .collect::<Vec<_>>(),
        [neutral, breakable]
    );
    assert!(!world.actor_view().movable(breakable));
    let player = world.player_id();
    let mut resolved = world.begin_step(dt(), idle(), 0).advance(&[], &[], &mut ());
    assert_eq!(resolved.damage(player, breakable, hp(2)), Some(5));
    resolved.finish();
    assert_eq!(world.max_health(breakable), Some(7));
}

#[test]
fn source_and_queued_births_receive_motors_on_their_first_tick() {
    let mut world = World::empty();
    let ticket = world
        .request_spawn(Vec2::new(-10.0, 0.0), Template::BODY)
        .unwrap();
    let source = world.add_source(
        Source::try_from(SourceSpec {
            pos: (10.0, 0.0),
            radius: 0.0,
            every: 1,
            when: Condition::FewerThan(1),
            what: Template::BODY,
            enabled: true,
        })
        .unwrap(),
    );
    let prepared = world.begin_step(dt(), idle(), 0);
    let outcomes = prepared.spawn_outcomes();
    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].ticket, ticket);
    assert_eq!(outcomes[1].source, Some(source));
    let queued = outcomes[0].entity.unwrap();
    let emitted = outcomes[1].entity.unwrap();
    let commands = [queued, emitted].map(|id| MotorCommand::new(id, Vec2::X, 30.0).unwrap());
    prepared.advance(&commands, &[], &mut ()).finish();
    for id in [queued, emitted] {
        assert_eq!(world.motion(id).unwrap().velocity(), Vec2::X * 0.5);
    }
    // The motor's first change ramps in over four substeps: 5/8 of it moves the body.
    assert!(
        (world.actor_view().position(queued).unwrap().x + 10.0 - 0.5 * 0.625 / 60.0).abs()
            < 0.00001
    );
    // Gameplay's explicit population prevents the next emission even though
    // the neutral source products have no damageable membership.
    world
        .begin_step(dt(), idle(), 1)
        .advance(&[], &[], &mut ())
        .finish();
    assert_eq!(world.body_count(), 2);
}

#[test]
fn explicit_effects_reject_dead_and_recycled_identities_and_preserve_player_body() {
    let mut world = World::empty();
    let actor = world
        .place(Vec2::new(10.0, 0.0), Template::BODY.with_health(hp(1)))
        .unwrap();
    let player = world.player_id();
    let mut resolved = world.begin_step(dt(), idle(), 0).advance(&[], &[], &mut ());
    assert_eq!(resolved.damage(player, actor, hp(1)), Some(0));
    assert!(!resolved.view().alive(actor));
    assert_eq!(
        resolved.damage(actor, player, hp(1)),
        None,
        "lethal damage prevents a trade"
    );
    assert_eq!(
        resolved.damage(player, actor, hp(1)),
        None,
        "duplicate lethal effects are harmless"
    );
    resolved.finish();
    assert_eq!(world.defeats()[0].body, actor);
    let replacement = world
        .place(Vec2::new(10.0, 0.0), Template::BODY.with_health(hp(1)))
        .unwrap();
    assert_ne!(actor, replacement);
    let mut resolved = world.begin_step(dt(), idle(), 0).advance(&[], &[], &mut ());
    assert_eq!(resolved.damage(actor, player, hp(1)), None);
    assert_eq!(resolved.damage(player, actor, hp(1)), None);
    assert_eq!(resolved.damage(replacement, player, hp(5)), Some(0));
    resolved.finish();
    assert_eq!(world.health(player), Some(0));
    assert!(world.actor_view().position(player).is_some());
    assert!(
        world.defeats().is_empty(),
        "the persistent player is retained at zero health"
    );
    assert!(world.trace().iter().any(|(_, event)| event
        == Event::Damaged {
            actor: replacement,
            target: player,
            remaining: 0
        }));
}

#[test]
fn generic_interaction_validates_actor_endpoint_and_distance() {
    let mut world = World::empty();
    let actor = world.place(Vec2::new(10.0, 0.0), Template::BODY).unwrap();
    let endpoint = world
        .place(Vec2::new(11.0, 0.0), Template::BLOCK.interactive())
        .unwrap();
    let far = world
        .place(Vec2::new(13.0, 0.0), Template::BLOCK.interactive())
        .unwrap();
    let mut resolved = world.begin_step(dt(), idle(), 0).advance(&[], &[], &mut ());
    assert!(!resolved.view().can_interact(actor, far));
    assert!(!resolved.interact(actor, far));
    assert!(resolved.view().can_interact(actor, endpoint));
    assert!(resolved.interact(actor, endpoint));
    assert!(!resolved.view().can_interact(actor, endpoint));
    assert!(!resolved.interact(actor, endpoint));
    resolved.finish();
    assert_eq!(world.actor_view().interactions().count(), 2);
}

#[test]
fn motors_reject_invalid_values_and_do_not_drive_stale_or_fixed_bodies() {
    let mut world = World::empty();
    let fixed = world.place(Vec2::new(10.0, 0.0), Template::BLOCK).unwrap();
    let retired = world.place(Vec2::new(20.0, 0.0), Template::BODY).unwrap();
    assert!(world.despawn_body(retired));
    let replacement = world.place(Vec2::new(20.0, 0.0), Template::BODY).unwrap();
    for velocity in [
        Vec2::splat(f32::NAN),
        Vec2::splat(f32::INFINITY),
        Vec2::X * 30.1,
    ] {
        assert!(MotorCommand::new(replacement, velocity, 30.0).is_err());
    }
    for acceleration in [f32::NAN, f32::INFINITY, -1.0, 300.1] {
        assert!(MotorCommand::new(replacement, Vec2::X, acceleration).is_err());
    }
    let commands = [fixed, retired].map(|id| MotorCommand::new(id, Vec2::X, 30.0).unwrap());
    world
        .begin_step(dt(), idle(), 0)
        .advance(&commands, &[], &mut ())
        .finish();
    assert_eq!(world.motion(fixed).unwrap().velocity(), Vec2::ZERO);
    assert_eq!(world.motion(replacement).unwrap().velocity(), Vec2::ZERO);
}

#[test]
fn controlled_attacks_damage_only_the_explicit_target_set() {
    let mut world = World::empty();
    world.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
    let eligible = world
        .place(Vec2::new(0.0, 2.0), Template::BLOCK.with_health(hp(3)))
        .unwrap();
    let excluded = world
        .place(Vec2::new(0.0, 3.0), Template::BLOCK.with_health(hp(3)))
        .unwrap();
    world
        .begin_step(dt(), idle().with_attack_held(true), 1)
        .advance(&[], &[eligible], &mut ())
        .finish();
    assert_eq!(world.health(eligible), Some(2));
    assert_eq!(world.health(excluded), Some(3));
    let player = world.player_id();
    assert!(world.trace().iter().any(|(_, event)| event
        == Event::Damaged {
            actor: player,
            target: eligible,
            remaining: 2
        }));
}

#[test]
fn controlled_and_autonomous_interaction_share_cover_admission() {
    let mut world = World::empty();
    let actor = world.place(Vec2::new(10.0, 0.0), Template::BODY).unwrap();
    let near_player = world.place(Vec2::X, Template::BLOCK.interactive()).unwrap();
    let near_actor = world
        .place(Vec2::new(11.0, 0.0), Template::BLOCK.interactive())
        .unwrap();
    let scene = world
        .load_scene(&arpg_sim::Scene {
            walls: [0.5, 10.5]
                .map(|x| arpg_sim::Wall::new(Vec2::new(x, 0.0), Vec2::new(0.1, 2.0), 0.0).unwrap())
                .to_vec(),
            ..Default::default()
        })
        .unwrap();
    let mut resolved = world
        .begin_step(dt(), idle().with_interact(true), 0)
        .advance(&[], &[], &mut ());
    assert_eq!(
        resolved.view().interaction(near_player),
        Some(arpg_sim::InteractionState::Ready)
    );
    assert!(!resolved.view().can_interact(actor, near_actor));
    assert!(!resolved.interact(actor, near_actor));
    resolved.finish();
    assert!(world.evict_scene(scene));
    let mut resolved = world
        .begin_step(dt(), idle().with_interact(true), 0)
        .advance(&[], &[], &mut ());
    assert_eq!(
        resolved.view().interaction(near_player),
        Some(arpg_sim::InteractionState::Activated)
    );
    assert!(resolved.interact(actor, near_actor));
    resolved.finish();
}

#[test]
fn protected_player_can_keep_playing_at_zero_health() {
    let mut world = World::empty();
    let actor = world
        .place(Vec2::new(10.0, 0.0), Template::BODY.with_health(hp(3)))
        .unwrap();
    let endpoint = world.place(Vec2::X, Template::BLOCK.interactive()).unwrap();
    let player = world.player_id();
    let mut resolved = world.begin_step(dt(), idle(), 0).advance(&[], &[], &mut ());
    assert_eq!(resolved.damage(actor, player, hp(5)), Some(0));
    assert!(!resolved.view().alive(player));
    assert!(resolved.view().can_act(player));
    assert_eq!(resolved.damage(player, actor, hp(1)), Some(2));
    assert!(resolved.interact(player, endpoint));
    resolved.finish();
    world.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
    let target = world
        .place(Vec2::new(0.0, 2.0), Template::BLOCK.with_health(hp(3)))
        .unwrap();
    world
        .begin_step(dt(), idle().with_attack_held(true), 1)
        .advance(&[], &[target], &mut ())
        .finish();
    assert_eq!(world.health(target), Some(2));
}

#[test]
fn numeric_position_admission_is_shared_and_preserves_accepted_work() {
    let mut world = World::empty();
    let ticket = world
        .request_spawn(Vec2::new(120.0, 0.0), Template::BODY)
        .unwrap();
    let before = world.hash();
    for position in [
        Vec2::splat(f32::MAX),
        Vec2::new(4.1e18, 0.0),
        Vec2::splat(f32::INFINITY),
    ] {
        assert!(!arpg_sim::valid_position(position));
        assert!(world.place(position, Template::BODY).is_none());
        assert!(world.request_spawn(position, Template::BODY).is_none());
        assert_eq!(
            world.hash(),
            before,
            "invalid requests must not consume tickets or change accepted work"
        );
        let scene = arpg_sim::Scene {
            bodies: vec![arpg_sim::Placed {
                pos: position.into(),
                what: Template::BODY,
            }],
            ..Default::default()
        };
        assert!(world.load_scene(&scene).is_err());
        assert_eq!(world.hash(), before);
    }
    let invalid_grid = arpg_sim::Scene {
        grids: vec![arpg_sim::BodyGrid {
            origin: (3.0e18, 0.0),
            columns: 2,
            rows: 1,
            spacing: 2.0e18,
            what: Template::BODY,
        }],
        ..Default::default()
    };
    assert!(world.load_scene(&invalid_grid).is_err());
    assert_eq!(world.hash(), before);
    assert!(
        Source::try_from(SourceSpec {
            pos: (3.0e18, 0.0),
            radius: 2.0e18,
            every: 1,
            when: Condition::Always,
            what: Template::BODY,
            enabled: true
        })
        .is_err()
    );
    let positive = Vec2::splat(4.0e18);
    let negative = -positive;
    assert!(arpg_sim::valid_position(positive) && arpg_sim::valid_position(negative));
    assert!(positive.distance_squared(negative).is_finite());
    assert!(arpg_sim::stopping_speed(positive.distance(negative), 30.0, dt(), 300.0).is_finite());
    world.place(positive, Template::BODY).unwrap();
    world.place(negative, Template::BODY).unwrap();
    world
        .begin_step(dt(), idle(), 0)
        .advance(&[], &[], &mut ())
        .finish();
    assert_eq!(world.spawn_outcomes()[0].ticket, ticket);
    let admitted = world.spawn_outcomes()[0].entity.unwrap();
    assert!(world.actor_view().position(admitted).unwrap().x <= arpg_sim::ARENA_HALF);
    assert!(world.all_positions_finite());
}
