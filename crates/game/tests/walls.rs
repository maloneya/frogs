//! Walls composed through the same public Game used by app and scenarios.
use arpg_game::{Scene, ActorDefinition};
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{Game, GameScene};
use arpg_sim::{Accumulator, AttackProfile, Beam, Dt, ENEMY_RADIUS, Impulse, MAX_WALLS, PLAYER_RADIUS, Wall};
use glam::{Vec2, Vec3};

fn wall(centre: Vec2, size: Vec2, yaw: f32) -> Wall {
    Wall::new(centre, size, yaw).unwrap()
}
fn scene(walls: Vec<Wall>) -> GameScene {
    Scene {
        walls,
        ..Scene::default()
    }
    .into()
}
fn step(game: &mut Game, movement: Vec3, aim: Vec3, attack: bool, held: bool) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(
        dt,
        Intent::new(MoveDir::new(movement), AimDir::new(aim).unwrap(), attack)
            .with_attack_held(held),
    );
}
fn report(game: &Game) -> String {
    let mut report = Report::default();
    game.report(&mut report);
    report.finish()
}

#[test]
fn wall_admission_is_atomic_and_lifetime_preserves_stable_scene_indices() {
    let content = scene(vec![wall(Vec2::new(0.0, 2.0), Vec2::new(4.0, 0.2), 0.0)]);
    let mut game = Game::from_scene(&content).unwrap();
    let first = game.walls().next().unwrap().0;
    let second = game.load_scene(&content).unwrap();
    assert!(game.evict_scene(first));
    assert!(!game.evict_scene(first));
    let survivor = game.walls().next().unwrap();
    assert_eq!((survivor.0, survivor.1), (second, 0));
    assert!(report(&game).contains(&format!("\"{second}/0\"")));
    assert!(game.request_spawn(Vec2::new(10.0, 0.0), ActorDefinition::passive_hostile()).is_some());
    let mut control = Game::from_scene(&content).unwrap();
    let control_first = control.walls().next().unwrap().0;
    control.load_scene(&content).unwrap();
    assert!(control.evict_scene(control_first));
    assert!(control.request_spawn(Vec2::new(10.0, 0.0), ActorDefinition::passive_hostile()).is_some());
    let before = (game.hash(), report(&game), game.trace().render());
    let full = scene(vec![content.engine.walls[0]; MAX_WALLS]);
    assert!(game.load_scene(&full).is_err());
    assert_eq!((game.hash(), report(&game), game.trace().render()), before);
    step(&mut game, Vec3::ZERO, Vec3::Z, false, false);
    step(&mut control, Vec3::ZERO, Vec3::Z, false, false);
    assert_eq!(
        game.hash(),
        control.hash(),
        "refusal preserves accepted pending work"
    );
    assert!(game.evict_scene(second));
    let next = game.load_scene(&content).unwrap();
    assert_ne!(next, first);
    assert!(
        !game.evict_scene(first),
        "stale scene cannot remove new walls"
    );
    game.restart().unwrap();
    assert_eq!(game.hash(), Game::from_scene(&content).unwrap().hash());
    assert_eq!(game.walls().count(), 1);
    game.start_scene(&GameScene::default()).unwrap();
    assert_eq!(game.walls().count(), 0);
}

#[test]
fn every_authored_wall_property_reaches_hash_and_observation_is_read_only() {
    let base = wall(Vec2::new(0.0, 2.0), Vec2::new(4.0, 0.2), 0.0);
    let game = Game::from_scene(&scene(vec![base])).unwrap();
    let before = game.hash();
    assert_eq!(game.walls().count(), 1);
    assert!(report(&game).contains("\"walls\""));
    assert_eq!(before, game.hash());
    for changed in [
        wall(Vec2::new(1.0, 2.0), base.size(), 0.0),
        wall(base.centre(), Vec2::new(5.0, 0.2), 0.0),
        wall(base.centre(), Vec2::new(4.0, 0.3), 0.0),
        wall(base.centre(), base.size(), 0.2),
    ] {
        assert_ne!(
            before,
            Game::from_scene(&scene(vec![changed])).unwrap().hash()
        );
    }
}

#[test]
fn rotated_face_and_joined_corner_stop_walk_and_leave_tangent_free() {
    let mut game = Game::from_scene(&scene(vec![wall(
        Vec2::new(0.0, 2.0),
        Vec2::new(8.0, 0.2),
        std::f32::consts::FRAC_PI_4,
    )]))
    .unwrap();
    for _ in 0..30 {
        step(&mut game, Vec3::new(1.0, 0.0, 1.0), Vec3::Z, false, false);
    }
    // The motor's 60N lean on the face is a light load: the contact holds it
    // rigidly within the half-millimetre slop, at the rotated face.
    let expected = 1.0 - 0.4 / 2.0_f32.sqrt();
    assert!(
        game.player_pos().abs_diff_eq(Vec3::new(expected, 0.6, expected), 0.0005),
        "{:?} vs {expected}",
        game.player_pos()
    );
    let mut game = Game::from_scene(&scene(vec![
        wall(Vec2::new(0.0, 2.0), Vec2::new(8.0, 0.2), 0.0),
        wall(Vec2::new(2.0, 0.0), Vec2::new(0.2, 8.0), 0.0),
    ]))
    .unwrap();
    for _ in 0..40 {
        step(&mut game, Vec3::new(1.0, 0.0, 1.0), Vec3::Z, false, false);
    }
    let resting = 1.6;
    assert!(
        game.player_pos().abs_diff_eq(Vec3::new(resting, 0.6, resting), 0.0005),
        "{:?} vs {resting}",
        game.player_pos()
    );
    // Moving west leaves the X face freely and slides along Z without adhering.
    let mut previous = game.player_pos();
    for _ in 0..5 {
        step(&mut game, -Vec3::X, Vec3::Z, false, false);
        let current = game.player_pos();
        assert!(current.x < previous.x, "the motor leaves the face");
        assert!(current.z <= previous.z + 1e-6, "the released face must not pull");
        previous = current;
    }
    assert!(game.player_pos().z <= 1.6 + 1e-6, "the released face must not adhere");
}

#[test]
fn crowd_correction_cannot_push_a_body_through_a_wall() {
    let mut game = Game::from_scene(&scene(vec![wall(
        Vec2::new(0.0, 2.0),
        Vec2::new(8.0, 0.01),
        0.0,
    )]))
    .unwrap();
    let front = game.place(Vec2::new(0.0, 1.74), ActorDefinition::passive_hostile()).unwrap();
    let rear = game.place(Vec2::new(0.0, 1.3), ActorDefinition::passive_hostile()).unwrap();
    assert!(game.apply_impulse(rear, Impulse::try_from((0.0, 20.0)).unwrap()));
    // Soft contacts may compress by millimetres, never through the thin wall's
    // near face at 2 - .01/2, even under the incoming crowd impulse.
    let enemy_limit = 1.995 - ENEMY_RADIUS + 0.01;
    let player_limit = 1.995 - PLAYER_RADIUS + 0.01;
    for _ in 0..20 {
        step(&mut game, Vec3::Z, Vec3::Z, false, false);
        assert!(game.body_pos(front).unwrap().z <= enemy_limit + 0.0001);
        assert!(game.body_pos(rear).unwrap().z <= enemy_limit + 0.0001);
        assert!(game.player_pos().z <= player_limit + 0.0001);
    }
}

#[test]
fn every_attack_obeys_cover_but_exposed_targets_remain_hittable() {
    for profile in [
        AttackProfile::Cleave,
        AttackProfile::Slam,
        AttackProfile::Disintegrate,
    ] {
        let content = scene(vec![wall(Vec2::new(0.0, 0.8), Vec2::new(4.0, 0.1), 0.0)]);
        let mut game = Game::from_scene(&content).unwrap();
        let target = game.place(Vec2::new(0.0, 1.3), ActorDefinition::passive_hostile()).unwrap();
        game.set_attack_profile(profile);
        for tick in 0..30 {
            step(
                &mut game,
                Vec3::ZERO,
                Vec3::Z,
                tick == 0,
                profile == AttackProfile::Disintegrate,
            );
        }
        assert_eq!(
            game.health(target),
            Some(3),
            "{profile:?} must respect cover"
        );
        assert_eq!(game.motion(target).unwrap().velocity(), Vec2::ZERO);
        let owner = game.walls().next().unwrap().0;
        assert!(game.evict_scene(owner));
        for tick in 0..30 {
            step(
                &mut game,
                Vec3::ZERO,
                Vec3::Z,
                tick == 0,
                profile == AttackProfile::Disintegrate,
            );
        }
        assert!(
            game.health(target).is_none_or(|health| health < 3),
            "{profile:?} must hit without cover"
        );
    }
}

#[test]
fn beam_endpoint_uses_nearest_rotated_wall_and_survives_origin_inside_a_wall() {
    let mut game = Game::from_scene(&scene(vec![
        wall(Vec2::new(0.0, 6.0), Vec2::new(4.0, 0.4), 0.0),
        wall(
            Vec2::new(0.0, 3.0),
            Vec2::new(4.0, 0.4),
            std::f32::consts::FRAC_PI_4,
        ),
    ]))
    .unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    step(&mut game, Vec3::ZERO, Vec3::Z, false, true);
    assert!((game.beam().unwrap().end().y - (3.0 - 0.2 * 2.0_f32.sqrt())).abs() < 0.00001);
    // Adding a wall under the already active beam must give a finite zero beam
    // immediately, before the next movement pass resolves penetration.
    game.load_scene(&scene(vec![wall(Vec2::ZERO, Vec2::ONE, 0.0)]))
        .unwrap();
    assert_eq!(game.beam().unwrap().start(), game.beam().unwrap().end());
    step(&mut game, Vec3::ZERO, Vec3::Z, false, true);
    assert!(game.beam().unwrap().end().is_finite());
    assert!(
        game.beam()
            .unwrap()
            .start()
            .distance(game.beam().unwrap().end())
            <= Beam::LENGTH
    );
}

#[test]
fn seeker_crowd_cannot_cross_the_face_while_compressing() {
    let mut content = scene(vec![wall(Vec2::new(0.0, 2.0), Vec2::new(40.0, 0.4), 0.0)]);
    content.engine.grids.push(arpg_game::BodyGrid {
        origin: (-10.85, 4.0),
        columns: 32,
        rows: 32,
        spacing: 0.7,
        what: ActorDefinition::hostile(),
    });
    let mut game = Game::from_scene(&content).unwrap();
    let ids = game
        .scene_bodies(game.walls().next().unwrap().0)
        .unwrap()
        .to_vec();
    // A 32-deep crowd leans on its front rank; the wall's stiffer static
    // contact holds that load within a few centimetres of its face.
    let far_face_limit = 2.2 + ENEMY_RADIUS - 0.05;
    for tick in 0..360 {
        step(&mut game, Vec3::ZERO, Vec3::Z, false, false);
        for &id in &ids {
            let position = game.body_pos(id).unwrap();
            assert!(position.z >= far_face_limit - 0.0001,
                "tick {tick}, {id}: {position:?} crosses the compression limit {far_face_limit}");
        }
    }
}

#[test]
fn reported_beam_endpoint_matches_the_clipped_game_observation() {
    let mut game = Game::from_scene(&scene(vec![wall(
        Vec2::new(0.0, 2.0),
        Vec2::new(4.0, 0.4),
        0.0,
    )]))
    .unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    step(&mut game, Vec3::ZERO, Vec3::Z, false, true);
    assert!(
        game.beam()
            .unwrap()
            .end()
            .abs_diff_eq(Vec2::new(0.0, 1.8), 0.00001)
    );
    assert!(report(&game).contains("\"end\":[0.0000,0.0000,1.8000]"));
}

#[test]
fn wall_recovery_at_an_arena_edge_chooses_an_in_bounds_exit() {
    let mut game = Game::empty();
    let body = game.place(Vec2::new(95.5, 0.0), ActorDefinition::passive_hostile()).unwrap();
    game.load_scene(&scene(vec![wall(
        Vec2::new(95.0, 0.0),
        Vec2::new(2.0, 8.0),
        0.0,
    )]))
    .unwrap();
    // The nearer +X face would eject the body beyond x=96. It leaves by -X,
    // at no more than the overlap push speed, and comes to rest outside.
    for _ in 0..60 {
        step(&mut game, Vec3::ZERO, Vec3::Z, false, false);
        assert!(game.body_pos(body).unwrap().x <= arpg_sim::ARENA_HALF - 0.25);
    }
    assert!((game.body_pos(body).unwrap().x - 93.75).abs() < 0.001);
}

#[test]
fn read_only_wall_segments_share_nearest_geometry_validation_and_scene_lifetime() {
    let mut game = Game::empty();
    let far = game
        .load_scene(&scene(vec![wall(
            Vec2::new(0.0, 4.0),
            Vec2::new(2.0, 0.2),
            0.0,
        )]))
        .unwrap();
    let near = game
        .load_scene(&scene(vec![wall(
            Vec2::new(0.0, 2.0),
            Vec2::new(2.0, 0.2),
            0.0,
        )]))
        .unwrap();
    let before = game.hash();
    let hit = game
        .solid_hit(Vec2::ZERO, Vec2::new(0.0, 6.0))
        .unwrap()
        .unwrap();
    assert!(hit.point().abs_diff_eq(Vec2::new(0.0, 1.9), 0.00001));
    assert!(hit.normal().abs_diff_eq(Vec2::NEG_Y, 0.00001));
    assert!((hit.fraction() - 1.9 / 6.0).abs() < 0.00001);
    let inside = game
        .solid_hit(Vec2::new(0.0, 2.0), Vec2::new(0.0, 6.0))
        .unwrap()
        .unwrap();
    assert_eq!(inside.fraction(), 0.0);
    assert_eq!(inside.normal(), Vec2::ZERO);
    for invalid in [
        Vec2::splat(f32::NAN),
        Vec2::splat(f32::INFINITY),
        Vec2::splat(f32::MAX),
    ] {
        assert!(game.solid_hit(invalid, Vec2::ZERO).is_err());
        assert!(game.solid_hit(Vec2::ZERO, invalid).is_err());
    }
    assert_eq!(
        game.hash(),
        before,
        "queries never mutate state, including refused input"
    );
    assert!(game.evict_scene(near));
    assert!(
        (game
            .solid_hit(Vec2::ZERO, Vec2::new(0.0, 6.0))
            .unwrap()
            .unwrap()
            .point()
            .y
            - 3.9)
            .abs()
            < 0.00001
    );
    assert!(game.evict_scene(far));
    assert!(
        game.solid_hit(Vec2::ZERO, Vec2::new(0.0, 6.0))
            .unwrap()
            .is_none()
    );
}
