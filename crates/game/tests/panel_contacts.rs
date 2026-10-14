//! Contacts discovered by rotation must exchange momentum without tunnelling;
//! public impulse admission must preserve the solver's finite-state invariant.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{DoorPanel, ActorDefinition, DoorSpec, Game, GameScene, Placed};
use arpg_sim::{Accumulator, Dt, Impulse, LockState, Mass, Template, Wall};
use glam::Vec2;

fn scene(thickness: f32) -> GameScene {
    let mut setup = GameScene::default();
    setup.engine.bodies.push(Placed {
        pos: (0.0, 4.0),
        what: prop(),
    });
    setup.doors.push(DoorSpec {
        body: 0,
        panel: DoorPanel::new(2.0, thickness, 0.0, -1.55, 1.55, 0.1).unwrap(),
    });
    setup
}
fn prop() -> ActorDefinition {
    ActorDefinition {
        physical: Template::BODY.with_mass(Mass::try_from(2.0).unwrap()),
        ..ActorDefinition::prop()
    }
}
fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}
fn ids(game: &Game) -> Vec<arpg_sim::EntityId> {
    let owner = game.scene_instances().next().unwrap().0;
    game.scene_bodies(owner).unwrap().to_vec()
}
fn observed(game: &Game) -> (u64, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish())
}
#[test]
fn accepted_point_impulses_cannot_overflow_latch_reactions() {
    let mut game = Game::from_scene(&scene(0.2)).unwrap();
    let panel = ids(&game)[0];
    let before = observed(&game);
    let impulse = Impulse::try_from((0.0, -1e19)).unwrap();
    // Both arguments are finite and individually valid. Their torque would
    // overflow downstream angular momentum / point-speed arithmetic.
    for _ in 0..10 {
        assert!(!game.apply_impulse_at(panel, Vec2::new(4e18, 4.0), impulse));
        assert_eq!(observed(&game), before);
    }
    step(&mut game);
    assert!(game.all_positions_finite());
    assert_eq!(game.joint(panel).unwrap().reaction_impulse(), 0.0);
}
#[test]
fn rotation_into_a_separated_body_transfers_momentum_in_the_same_tick() {
    let mut setup = scene(0.2);
    setup.engine.bodies.push(Placed {
        pos: (0.5, 3.64),
        what: prop(),
    });
    let mut game = Game::from_scene(&setup).unwrap();
    let ids = ids(&game);
    // Initial normal gap is .01m. The free rotation would advance the panel
    // .009975rad and overlap this initially separated body by about .005m.
    assert!(game.apply_impulse_at(
        ids[0],
        Vec2::new(1.0, 4.0),
        Impulse::try_from((0.0, -0.8)).unwrap()
    ));
    step(&mut game);
    let panel = game.joint(ids[0]).unwrap();
    let body = game.body_pos(ids[1]).unwrap();
    let normal = Vec2::new(panel.collider().yaw().sin(), panel.collider().yaw().cos());
    let distance = normal.dot(Vec2::new(body.x, body.z) - panel.collider().centre()).abs();
    assert!(
        distance >= 0.35 - 0.005,
        "panel overlapped the body beyond the soft contact bound: {distance}"
    );
    assert!(
        game.motion(ids[1]).unwrap().velocity().y < 0.0,
        "contact must transfer momentum, not only repair position"
    );
    // The fixed hinge can absorb linear momentum but cannot supply energy.
    let inertia = 2.0 * (2.0_f32.powi(2) / 3.0 + 0.2_f32.powi(2) / 12.0);
    let initial_energy = (2.0_f32 * 0.8).powi(2) / (2.0 * inertia);
    let final_energy = 0.5 * inertia * panel.rate().powi(2)
        + game.motion(ids[1]).unwrap().velocity().length_squared();
    assert!(final_energy <= initial_energy + 1e-4,
        "contact created energy: {initial_energy} -> {final_energy}");
    assert!(game.all_positions_finite());
}
#[test]
fn rotating_thin_panel_cannot_skip_a_thin_static_wall() {
    let mut setup = scene(0.01);
    setup
        .engine
        .walls
        .push(Wall::new(Vec2::new(0.95, 3.94), Vec2::splat(0.01), 0.0).unwrap());
    let mut game = Game::from_scene(&setup).unwrap();
    let id = ids(&game)[0];
    assert!(game.apply_impulse_at(
        id,
        Vec2::new(1.0, 4.0),
        Impulse::try_from((0.0, -4.8)).unwrap()
    ));
    step(&mut game);
    // Free rotation is .06rad, with only .12m free-edge travel. It crosses the
    // wall's centre near .03076rad, so sampling by body radius would skip it.
    let angle = game.joint(id).unwrap().angle();
    assert!(
        angle > 0.0 && angle < 0.03,
        "panel crossed the thin wall: {angle}"
    );
    assert!(game.all_positions_finite());
}

#[test]
fn a_fast_disc_cannot_cross_a_latched_panel_between_endpoint_samples() {
    let mut setup = scene(0.01);
    setup.doors[0].panel = DoorPanel::new(2.0, 0.01, 0.0, -1.55, 1.55, 1e9).unwrap();
    setup.engine.bodies.push(Placed {
        pos: (0.0, 3.0),
        what: ActorDefinition::passive_hostile(),
    });
    let mut game = Game::from_scene(&setup).unwrap();
    let ids = ids(&game);
    // Free travel is over 16m, far beyond what collision passes can follow,
    // ending entirely beyond the .01m panel. The continuous sweep stops it.
    assert!(game.apply_impulse(ids[1], Impulse::try_from((0.0, 1000.0)).unwrap()));
    step(&mut game);
    assert_eq!(game.joint(ids[0]).unwrap().lock(), LockState::Held);
    assert!(game.body_pos(ids[1]).unwrap().z <= 4.0 - 0.005 - arpg_sim::ENEMY_RADIUS + 1e-4,
        "swept body crossed the panel: {:?}", game.body_pos(ids[1]));
    assert!(game.motion(ids[1]).unwrap().velocity().y <= 1e-5);
    assert!(game.all_positions_finite());
}

#[test]
fn a_fast_body_stops_at_a_latched_panel_surface_and_loads_the_latch() {
    let mut setup = scene(0.2);
    setup.doors[0].panel = DoorPanel::new(2.0, 0.2, 0.0, -1.55, 1.55, 1e6).unwrap();
    setup.engine.bodies.push(Placed {
        pos: (0.0, 3.4),
        what: ActorDefinition {
            physical: Template::BODY,
            ..ActorDefinition::prop()
        },
    });
    let mut game = Game::from_scene(&setup).unwrap();
    let ids = ids(&game);
    // .3m of travel toward a surface .25m away: the speculative contact lets
    // the body close the gap, then stops it there within the same tick.
    assert!(game.apply_impulse(ids[1], Impulse::try_from((0.0, 18.0)).unwrap()));
    step(&mut game);
    let surface = 4.0 - 0.1 - arpg_sim::ENEMY_RADIUS;
    let z = game.body_pos(ids[1]).unwrap().z;
    assert!((z - surface).abs() < 0.001, "body stopped at {z}, surface at {surface}");
    assert!(game.motion(ids[1]).unwrap().velocity().y.abs() < 0.01);
    let panel = game.joint(ids[0]).unwrap();
    assert_eq!(panel.angle(), 0.0);
    assert_eq!(panel.lock(), LockState::Held);
    // The latch absorbs all 18kg m/s at the panel's midpoint: half at the edge.
    assert!((panel.reaction_impulse() - 9.0).abs() < 0.05, "{}", panel.reaction_impulse());
}
