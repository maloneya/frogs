//! Hinges must react at the overlapping surface, and position repair cannot
//! bypass solid geometry or manufacture the momentum of an impact.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{
    ActorDefinition, DefeatObjective, DoorSpec, EncounterAction, EncounterObjective,
    EncounterSpec, EncounterStart, Game, GameScene, Placed, SourceSpec,
};
use arpg_sim::{Accumulator, Condition, Dt, EntityId, Impulse, LatchState, Mass, PanelConfig, Template, Wall};
use glam::Vec2;

fn prop() -> ActorDefinition {
    ActorDefinition {
        physical: Template::BODY.with_mass(Mass::try_from(2.0).unwrap()),
        ..ActorDefinition::prop()
    }
}
fn config(thickness: f32) -> PanelConfig {
    PanelConfig::new(2.0, thickness, 0.0, -1.55, 1.55, 12.0).unwrap()
}
fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}
fn released(panels: &[(Vec2, PanelConfig)], walls: Vec<Wall>) -> (Game, Vec<EntityId>) {
    let mut scene = GameScene::default();
    scene.engine.walls = walls;
    for &(centre, panel) in panels {
        let body = scene.engine.bodies.len();
        scene.engine.bodies.push(Placed { pos: centre.into(), what: prop() });
        scene.doors.push(DoorSpec { body, panel });
    }
    let trigger = scene.engine.bodies.len();
    scene.engine.bodies.push(Placed {
        pos: (1.4, 0.0), what: ActorDefinition::prop().interactive(),
    });
    scene.engine.sources.push(SourceSpec {
        pos: (30.0, 30.0), radius: 0.0, every: 10_000, when: Condition::Always,
        enabled: false, what: ActorDefinition::passive_hostile(),
    });
    scene.encounters.push(EncounterSpec {
        name: "Release panels".into(), start: EncounterStart::Activated(trigger),
        objective: EncounterObjective::Defeat(DefeatObjective {
            sources: vec![0], count: 1.try_into().unwrap(), within_ticks: 1000.try_into().unwrap(),
        }),
        on_start: (0..panels.len()).map(EncounterAction::UnlockDoor).collect(),
        on_success: vec![], on_failure: vec![],
    });
    let mut game = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let ids = game.scene_bodies(owner).unwrap()[..panels.len()].to_vec();
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_interact(true),
    );
    step(&mut game);
    for &id in &ids {
        let panel = game.panel(id).unwrap();
        assert_eq!(panel.latch(), LatchState::Released);
        assert_eq!(panel.angle(), 0.0);
        assert_eq!(panel.angular_velocity(), 0.0);
    }
    (game, ids)
}

#[test]
fn offset_panel_reacts_on_the_overlapping_end_of_a_long_wall() {
    let wall = Wall::new(Vec2::new(0.0, 3.9), Vec2::new(50.0, 0.2), 0.0).unwrap();
    let (mut game, ids) = released(&[(Vec2::new(10.0, 4.11), config(0.2))], vec![wall]);
    let id = ids[0];
    // The hinge is x=9. Contact near x=11 opposes positive rotation. Averaging
    // the unrelated wall and panel support centres would put it at x=5,
    // behind the hinge, and reverse the reaction's lever arm.
    assert!(game.apply_impulse_at(id, Vec2::new(11.0, 4.11), Impulse::try_from((0.0, -4.8)).unwrap()));
    step(&mut game);
    let panel = game.panel(id).unwrap();
    // The lower free corner reaches wall top z=4 when
    // 4.11 - 2*sin(angle) - .1*cos(angle) = 4, near .005001 rad.
    assert!(panel.angle() > 0.0 && panel.angle() < 0.006,
        "offset panel passed its supporting wall: {}", panel.angle());
    let lowest = panel.hinge().y - 2.0 * panel.angle().sin() - 0.1 * panel.angle().cos();
    assert!(lowest >= 4.0 - 0.00001, "panel penetrated wall: {lowest}");
    assert_eq!(panel.latch(), LatchState::Released);
    assert_eq!(panel.reaction_impulse(), 0.0);
    assert!(game.all_positions_finite());
}

#[test]
fn a_rotating_panel_transfers_momentum_to_a_second_panel_without_overlap() {
    let (mut game, ids) = released(&[
        (Vec2::new(0.0, 4.0), config(0.1)),
        (Vec2::new(0.0, 3.88), config(0.1)),
    ], vec![]);
    assert!(game.apply_impulse_at(ids[0], Vec2::new(1.0, 4.0), Impulse::try_from((0.0, -2.0)).unwrap()));
    let initial_omega = game.panel(ids[0]).unwrap().angular_velocity();
    step(&mut game);
    let a = game.panel(ids[0]).unwrap();
    let b = game.panel(ids[1]).unwrap();
    assert!(a.angle() > 0.0 && b.angle() > 0.0);
    assert!(a.angular_velocity() < initial_omega * 0.9);
    assert!(b.angular_velocity() > 0.0, "panel contact supplied no angular momentum");
    // Both panels have equal hinge inertia; contact and damping cannot add energy.
    assert!(a.angular_velocity().powi(2) + b.angular_velocity().powi(2)
        <= initial_omega.powi(2) + 0.00001);
    let axis_a = Vec2::new(a.yaw().cos(), -a.yaw().sin());
    let normal_a = Vec2::new(a.yaw().sin(), a.yaw().cos());
    let normal_b = Vec2::new(b.yaw().sin(), b.yaw().cos());
    // A's lower free corner is the first support to reach B's upper face.
    let corner = a.hinge() + 2.0 * axis_a - 0.05 * normal_a;
    let separation = (corner - b.centre()).dot(normal_b) - 0.05;
    assert!(separation >= -0.00001, "panels overlap at the striking corner: {separation}");
    for panel in [a, b] {
        assert_eq!(panel.latch(), LatchState::Released);
        assert_eq!(panel.reaction_impulse(), 0.0);
    }
    assert!(game.all_positions_finite());
}

#[test]
fn initial_overlap_recovery_cannot_rotate_a_panel_through_a_thin_wall() {
    let wall = Wall::new(Vec2::new(0.95, 3.94), Vec2::splat(0.01), 0.0).unwrap();
    let (mut game, ids) = released(&[(Vec2::new(0.0, 4.0), config(0.01))], vec![wall]);
    let panel_id = ids[0];
    let body = game.place(Vec2::new(0.0, 4.01), prop()).unwrap();
    // The initial disc overlap is .245m. An unconstrained angular position
    // correction exceeds .1rad, jumping across the distant wall near .03rad.
    // Recovery must move the movable body while respecting that intervening wall.
    // Soft correction closes overlap at the contact spring's rate: allow a second.
    for tick in 0..60 {
        step(&mut game);
        let panel = game.panel(panel_id).unwrap();
        assert!(panel.angle() >= 0.0 && panel.angle() < 0.03,
            "overlap recovery crossed the thin wall on tick {tick}: {}", panel.angle());
        // Overlap correction moves at no more than the push speed; drag then
        // stops what one relaxation sweep leaves behind.
        let push = game.tuning().get(arpg_sim::TuningField::ContactPushSpeed);
        assert!(panel.angular_velocity().abs() * 2.0 < push, "overlap correction spun the panel");
        assert!(game.motion(body).unwrap().velocity().length() < push);
        assert_eq!(panel.reaction_impulse(), 0.0, "position repair loaded the released latch");
        assert_eq!(panel.latch(), LatchState::Released);
        assert!(game.all_positions_finite());
    }
    let panel = game.panel(panel_id).unwrap();
    let position = game.body_pos(body).unwrap();
    let normal = Vec2::new(panel.yaw().sin(), panel.yaw().cos());
    let distance = (Vec2::new(position.x, position.z) - panel.centre()).dot(normal);
    assert!(distance >= 0.255 - 0.00001, "body remained inside the panel: {distance}");
}
