//! Mechanical failure is a constraint reaction, independent of source and health.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{DoorPanel, ActorDefinition, DoorSpec, DoorState, Game, GameScene, Placed};
use arpg_sim::{Accumulator, Dt, Impulse, Mass, Template};
use arpg_game::{AttackProfile, TuningField};
use glam::{Vec2, Vec3};

fn prop(mass: f32) -> ActorDefinition {
    ActorDefinition { physical: Template::BODY.with_mass(Mass::try_from(mass).unwrap()), ..ActorDefinition::prop() }
}
fn scene(strength: f32) -> GameScene {
    let mut scene = GameScene::default();
    scene.engine.bodies.push(Placed { pos: (0.0, 4.0), what: prop(2.0) });
    scene.doors.push(DoorSpec { body: 0, panel: DoorPanel::new(2.0, 0.2, 0.0, -1.55, 1.55, strength).unwrap() });
    scene
}
fn step(game: &mut Game, direction: Vec3, attack: bool) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::new(direction), AimDir::FORWARD, attack));
}
fn body(game: &Game) -> arpg_sim::EntityId {
    let owner = game.scene_instances().next().unwrap().0;
    game.scene_bodies(owner).unwrap()[0]
}
fn observed(game: &Game) -> (u64, String) {
    let mut report = Report::default(); game.report(&mut report); (game.hash(), report.finish())
}

#[test]
fn latch_uses_leverage_and_releasing_a_broken_constraint_preserves_motion() {
    for (point, should_break) in [(Vec2::new(0.0, 4.0), false), (Vec2::new(1.0, 4.0), true)] {
        let mut game = Game::from_scene(&scene(0.75)).unwrap();
        let id = body(&game);
        // One kg m/s applied halfway along the 2m panel needs 0.5 at its latch;
        // the same impulse at its far edge needs 1.0. Threshold0.75 separates them.
        assert!(game.apply_impulse_at(id, point, Impulse::try_from((0.0, -1.0)).unwrap()));
        step(&mut game, Vec3::ZERO, false);
        let panel = game.joint(id).unwrap();
        if should_break {
            assert_eq!(panel.lock(), arpg_sim::LockState::Broken);
            assert!(panel.angle() > 0.0, "breaching momentum must move the panel");
            assert!(panel.rate() > 0.0);
        } else {
            assert_eq!(panel.lock(), arpg_sim::LockState::Held);
            assert_eq!(panel.angle(), 0.0);
            assert!((panel.reaction_impulse() - 0.5).abs() < 1e-4);
        }
        assert!(game.is_alive(id));
    }
}

#[test]
fn subthreshold_impacts_do_not_accumulate_damage_and_overlap_is_not_load() {
    let mut game = Game::from_scene(&scene(1.0)).unwrap();
    let id = body(&game);
    for _ in 0..120 {
        assert!(game.apply_impulse(id, Impulse::try_from((0.0, -1.0)).unwrap()));
        step(&mut game, Vec3::ZERO, false);
        assert_eq!(game.joint(id).unwrap().lock(), arpg_sim::LockState::Held);
    }
    // A newly placed passive body intersects the panel. Geometry repair cannot
    // be interpreted as an impact, even when the latch is extremely weak.
    let mut authored = scene(0.001);
    authored.engine.bodies.push(Placed { pos: (0.0, 4.05), what: prop(1.0) });
    let mut game = Game::from_scene(&authored).unwrap();
    let id = body(&game);
    for _ in 0..30 { step(&mut game, Vec3::ZERO, false); }
    assert_eq!(game.joint(id).unwrap().lock(), arpg_sim::LockState::Held);
    assert_eq!(game.joint(id).unwrap().reaction_impulse(), 0.0);

    // Simultaneous opposite impulses cancel mechanically. Summing their absolute
    // magnitudes would invent a load and break this otherwise stationary panel.
    let mut game = Game::from_scene(&scene(0.1)).unwrap();
    let id = body(&game);
    for z in [-10.0, 10.0] {
        assert!(game.apply_impulse_at(id, Vec2::new(1.0, 4.0), Impulse::try_from((0.0, z)).unwrap()));
    }
    step(&mut game, Vec3::ZERO, false);
    assert_eq!(game.joint(id).unwrap().lock(), arpg_sim::LockState::Held);
    assert_eq!(game.joint(id).unwrap().reaction_impulse(), 0.0);
}

#[test]
fn player_push_and_attack_limits_are_tuning_not_immunity() {
    let mut authored = scene(100.0);
    authored.engine.bodies[0].pos = (0.0, 1.2);
    for strong in [false, true] {
        let mut game = Game::from_scene(&authored).unwrap();
        let id = body(&game);
        if strong {
            game.set_tuning(game.tuning().with(TuningField::PlayerAcceleration, 300.0).unwrap()
                .with(TuningField::PlayerMass, 100.0).unwrap());
        }
        for _ in 0..120 { step(&mut game, Vec3::Z, false); }
        assert_eq!(game.joint(id).unwrap().lock() == arpg_sim::LockState::Broken, strong);
    }
    for strong in [false, true] {
        let mut game = Game::from_scene(&authored).unwrap();
        let id = body(&game);
        game.set_attack_profile(AttackProfile::Slam);
        if strong { game.set_tuning(game.tuning().with(TuningField::SlamImpulse, 1000.0).unwrap()); }
        for tick in 0..120 { step(&mut game, Vec3::ZERO, tick % 40 == 0); }
        assert_eq!(game.joint(id).unwrap().lock() == arpg_sim::LockState::Broken, strong);
    }
}

#[test]
fn panel_admission_lifetime_and_replay_preserve_all_physical_state() {
    let authored = scene(0.75);
    let mut game = Game::from_scene(&authored).unwrap();
    let mut replay = Game::from_scene(&authored).unwrap();
    let id = body(&game);
    for world in [&mut game, &mut replay] {
        assert!(world.apply_impulse_at(body(world), Vec2::new(1.0, 4.0), Impulse::try_from((0.0, -1.0)).unwrap()));
    }
    for _ in 0..60 { step(&mut game, Vec3::ZERO, false); step(&mut replay, Vec3::ZERO, false); assert_eq!(game.hash(), replay.hash()); }
    let before = observed(&game);
    let mut acting_panel = authored.engine.bodies[0].what.clone();
    acting_panel.actions.push(arpg_game::ActionDefinition::Wait { ticks: 1 });
    acting_panel.behavior = arpg_game::BehaviorNode::Action(0);
    assert!(game.assign_behavior(id, acting_panel).is_err());
    assert_eq!(observed(&game), before);
    let mut invalid = authored.clone();
    invalid.engine.bodies[0].what.physical = Template::BLOCK;
    assert!(game.load_scene(&invalid).is_err()); assert_eq!(observed(&game), before);
    invalid = authored.clone(); invalid.doors.push(invalid.doors[0]);
    assert!(game.load_scene(&invalid).is_err()); assert_eq!(observed(&game), before);
    invalid = authored.clone(); invalid.engine.bodies[0].pos = (95.9, 95.9);
    assert!(game.load_scene(&invalid).is_err()); assert_eq!(observed(&game), before);
    assert!(!game.apply_impulse_at(id, Vec2::splat(f32::NAN), Impulse::try_from((0.0, 1.0)).unwrap()));
    assert_eq!(observed(&game), before);
    let owner = game.scene_instances().next().unwrap().0;
    assert!(game.despawn_body(id));
    assert_eq!(game.door_state(owner, 0), Some(DoorState::Missing));
    let replacement = game.place(Vec2::new(0.0, 4.0), prop(2.0)).unwrap();
    assert!(game.joint(replacement).is_none());
    assert!(!game.apply_impulse(id, Impulse::try_from((0.0, 10.0)).unwrap()));
    assert!(game.evict_scene(owner)); assert_eq!(game.door_state(owner, 0), None);
    game.restart().unwrap();
    assert_eq!(observed(&game), observed(&Game::from_scene(&authored).unwrap()));
}

#[test]
fn joint_capacity_is_atomic_and_eviction_returns_it() {
    let mut authored = GameScene::default();
    for index in 0..arpg_sim::MAX_JOINTS {
        authored.engine.bodies.push(Placed { pos: (-40.0 + (index % 16) as f32 * 5.0,
            -40.0 + (index / 16) as f32 * 5.0), what: prop(2.0) });
        authored.doors.push(DoorSpec { body: index, panel: DoorPanel::default() });
    }
    let mut game = Game::from_scene(&authored).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let before = observed(&game);
    let mut oversized = authored.clone();
    oversized.engine.bodies.push(Placed { pos: (40.0, 40.0), what: prop(2.0) });
    oversized.doors.push(DoorSpec { body: arpg_sim::MAX_JOINTS, panel: DoorPanel::default() });
    assert_eq!(game.load_scene(&oversized), Err(arpg_game::SceneError::Engine(arpg_sim::SceneError::Capacity)));
    assert_eq!(observed(&game), before);
    assert!(game.load_scene(&scene(1.0)).is_err());
    assert_eq!(observed(&game), before);
    assert!(game.evict_scene(owner));
    let added = game.load_scene(&scene(1.0)).unwrap();
    assert_eq!(game.door_state(added, 0), Some(DoorState::Latched));
    assert_eq!(game.joint_presentations(arpg_sim::Alpha::ONE).count(), 1);
}

#[test]
fn accumulating_crowd_transmits_load_breaks_the_latch_and_crosses_the_opening() {
    use arpg_game::{ActionDefinition, BehaviorNode, MoveAction, SourceSpec};
    use arpg_sim::{Condition, Wall};
    // A narrow, test-owned lane isolates transmission through contacts from
    // load diverted into a wide room's front wall. One mover supplies at most
    // 2.5kg m/s per tick; the centred latch receives half of that.
    let mut authored = scene(12.0);
    authored.engine.bodies[0].pos = (0.0, 3.0);
    authored.engine.bodies[0].what = prop(5.0);
    authored.doors[0].panel = DoorPanel::new(1.2, 0.12, 0.0, 0.0, 1.55, 12.0).unwrap();
    for (centre, size) in [((-0.8, 6.5), (0.2, 7.2)), ((0.8, 6.5), (0.2, 7.2)), ((0.0, 10.0), (1.8, 0.2))] {
        authored.engine.walls.push(Wall::new(centre.into(), size.into(), 0.0).unwrap());
    }
    let actor = ActorDefinition { physical: ActorDefinition::passive_hostile().physical.with_mass(Mass::try_from(5.0).unwrap()),
        actions: vec![ActionDefinition::Move(MoveAction::default())], behavior: BehaviorNode::Action(0),
        ..ActorDefinition::passive_hostile() };
    authored.engine.sources.push(SourceSpec { pos: (0.0, 7.0), radius: 0.0, every: 30,
        when: Condition::Always, enabled: true, what: actor });
    let mut game = Game::from_scene(&authored).unwrap();
    let id = body(&game);
    let mut breached = None;
    let mut max_load = 0.0_f32;
    let mut replay = Game::from_scene(&authored).unwrap();
    game.capture_events(4, vec![arpg_sim::EventKind::LockBroken], vec![]).unwrap();
    let mut stamp = None;
    for tick in 0..900 {
        let before = game.tick();
        step(&mut game, Vec3::ZERO, false);
        step(&mut replay, Vec3::ZERO, false);
        assert_eq!(game.hash(), replay.hash());
        assert!(game.all_positions_finite());
        let panel = game.joint(id).unwrap();
        max_load = max_load.max(panel.reaction_impulse());
        if tick < 120 { assert_eq!(panel.lock(), arpg_sim::LockState::Held); }
        if panel.lock() == arpg_sim::LockState::Broken && breached.is_none() {
            breached = Some((tick, game.enemy_count()));
            stamp = Some(before);
        }
    }
    let (tick, population) = breached.expect("connected crowd must exceed strength");
    // The breach is traced once, on the tick it happens.
    assert_eq!(game.captured_events().unwrap(), [(stamp.unwrap(), arpg_sim::Event::LockBroken { id })]);
    assert!((120..600).contains(&tick));
    // A failing latch spends exactly its strength; the excess opens the panel.
    assert!(population >= 3 && (max_load - 12.0).abs() < 1e-3, "max load {max_load}");
    let crossed = game.character_presentations(arpg_sim::Alpha::ONE).filter(|enemy| enemy.ground_position().z < 2.0).count();
    assert!(crossed >= 5, "the panel must leave a physical opening for the crowd; crossed={crossed}");
    assert_eq!(game.enemy_count(), 30, "sources continue through breach");
    assert!(game.is_alive(id), "breach retains the panel");
}
