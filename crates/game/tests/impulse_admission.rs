//! Impulse admission bounds resulting physical point speeds, independent of
//! gameplay role. Refusal preserves momentum, trace and accepted births.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{ActorDefinition, DoorSpec, Game, GameScene, Placed};
use arpg_sim::{Accumulator, Dt, EntityId, Impulse, LatchState, Mass, PanelConfig, Template};
use glam::Vec2;

fn prop(mass: f32) -> ActorDefinition {
    ActorDefinition {
        physical: Template::BODY.with_mass(Mass::try_from(mass).unwrap()),
        ..ActorDefinition::prop()
    }
}
fn observed(game: &Game) -> (u64, String, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish(), game.trace().render())
}
fn panel_game(thickness: f32) -> (Game, EntityId) {
    let mut scene = GameScene::default();
    scene.engine.bodies.push(Placed { pos: (0.0, 4.0), what: prop(2.0) });
    scene.doors.push(DoorSpec {
        body: 0, panel: PanelConfig::new(2.0, thickness, 0.0, -1.0, 1.0, 0.1).unwrap(),
    });
    let game = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let id = game.scene_bodies(owner).unwrap()[0];
    (game, id)
}
fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}

#[test]
fn linear_speed_admission_accounts_for_mass_carried_velocity_and_player_membership() {
    let mut game = Game::empty();
    let light = game.place(Vec2::new(10.0, 0.0), prop(0.1)).unwrap();
    let heavy = game.place(Vec2::new(20.0, 0.0), prop(10_000.0)).unwrap();
    let hostile = game.place(Vec2::new(30.0, 0.0), ActorDefinition::passive_hostile()).unwrap();
    let ticket = game.request_spawn(Vec2::new(40.0, 0.0), prop(1.0)).unwrap();
    for id in [game.player_id(), light, heavy, hostile] {
        let mass = game.motion(id).unwrap().inverse_mass().recip();
        let impulse = |speed: f32| Impulse::try_from((speed * mass, 0.0)).unwrap();
        assert!(game.apply_impulse(id, impulse(1_000_000.0)));
        assert!((game.motion(id).unwrap().velocity().x - 1_000_000.0).abs() < 0.1);
        let before = observed(&game);
        assert!(!game.apply_impulse(id, impulse(100.0)));
        assert!(!game.apply_impulse_at(id, Vec2::ZERO, impulse(100.0)));
        assert_eq!(observed(&game), before);
        // The input impulse itself can exceed the speed envelope: reversing
        // existing momentum leaves the resulting speed inside it.
        assert!(game.apply_impulse(id, impulse(-2_000_000.0)));
        assert!(game.apply_impulse(id, impulse(1_000_000.0)));
        assert!(game.motion(id).unwrap().velocity().length() < 0.1);
    }
    step(&mut game);
    assert!(game.spawn_outcomes().iter().any(|outcome| outcome.ticket == ticket && outcome.entity.is_some()));
}

#[test]
fn finite_extreme_panel_impulses_refuse_without_changing_accepted_work() {
    let (mut game, panel) = panel_game(0.2);
    let ticket = game.request_spawn(Vec2::new(20.0, 0.0), prop(1.0)).unwrap();
    let before = observed(&game);
    let impulse = Impulse::try_from((0.0, -1e18)).unwrap();
    assert!(!game.apply_impulse(panel, impulse));
    assert!(!game.apply_impulse_at(panel, Vec2::new(1.0, 4.0), impulse));
    assert_eq!(observed(&game), before);
    step(&mut game);
    assert_eq!(game.panel(panel).unwrap().latch(), LatchState::Latched);
    assert_eq!(game.panel(panel).unwrap().angle(), 0.0);
    assert!(game.spawn_outcomes().iter().any(|outcome| outcome.ticket == ticket && outcome.entity.is_some()));
}

#[test]
fn angular_admission_uses_the_fastest_corner_and_accumulated_velocity() {
    let (mut game, panel) = panel_game(2.0);
    // A 2m square of mass 2 has hinge inertia 10/3. A far-edge impulse of
    // 700000 produces omega=420000; its fastest corner travels sqrt(5)*omega.
    let edge = Vec2::new(1.0, 4.0);
    assert!(game.apply_impulse_at(panel, edge, Impulse::try_from((0.0, -700_000.0)).unwrap()));
    assert!((game.panel(panel).unwrap().angular_velocity() - 420_000.0).abs() < 0.1);
    let before = observed(&game);
    // Adding 100000 would give omega=480000. The far-edge midpoint still
    // travels only 960000m/s, but its corner exceeds 1000000m/s.
    assert!(!game.apply_impulse_at(panel, edge, Impulse::try_from((0.0, -100_000.0)).unwrap()));
    assert_eq!(observed(&game), before);
}

#[test]
fn a_large_admitted_panel_impulse_reaches_its_stop_and_remains_finite() {
    let (mut game, panel) = panel_game(0.2);
    assert!(game.apply_impulse_at(panel, Vec2::new(1.0, 4.0), Impulse::try_from((0.0, -100_000.0)).unwrap()));
    // Spin is capped so a corner never outruns one collision pass's reach
    // (0.16m per pass, eight passes): the swing takes a few ticks, not one.
    for _ in 0..3 {
        step(&mut game);
        let state = game.panel(panel).unwrap();
        assert_eq!(state.latch(), LatchState::Broken);
        assert!(state.angle() <= 1.0 && game.all_positions_finite());
    }
    // Stopping the swing leaves f32 roundoff of about 1e-3 rad/s, which drag
    // then decays; the panel stays within 1e-4 rad of its stop.
    let state = game.panel(panel).unwrap();
    assert!((state.angle() - 1.0).abs() < 0.0001);
    assert!(state.angular_velocity().abs() < 0.01);
}
