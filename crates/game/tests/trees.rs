//! Solid woodland content shares atomic admission and lifetime with gameplay.
use arpg_game::{ActorDefinition, TreeState};
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{Game, GameScene, MAX_TREES, TreeKind, TreeSpec};
use arpg_sim::{Accumulator, Dt, };
use glam::Vec2;

fn tree(kind: TreeKind) -> TreeSpec { TreeSpec::new(kind, Vec2::new(12.0, 3.0), 0.5, 1.0).unwrap() }
fn scene(kind: TreeKind, count: usize) -> GameScene {
    GameScene { trees: vec![tree(kind); count], ..GameScene::default() }
}
fn observed(game: &Game) -> (u64, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish())
}
fn step(game: &mut Game) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(), Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}
#[test]
fn scene_owned_trees_restore_on_restart_and_reject_stale_eviction() {
    let authored = scene(TreeKind::Pine, 2);
    let mut game = Game::from_scene(&authored).unwrap();
    let fresh = observed(&game);
    let first = game.scene_instances().next().unwrap().0;
    let second = game.load_scene(&scene(TreeKind::Bare, 1)).unwrap();
    step(&mut game);
    assert_eq!(game.trees().count(), 3);
    assert_eq!(game.walls().count(), 0, "trunks are bodies, not walls");
    assert_eq!(game.joint_presentations(arpg_sim::Alpha::ONE).count(), 3);
    let evicted = game.tree_body(first, 0).unwrap();
    assert!(game.evict_scene(first));
    assert!(game.joint(evicted).is_none());
    assert_eq!(game.tree_state(first, 0), None);
    let third = game.load_scene(&scene(TreeKind::Broadleaf, 1)).unwrap();
    assert!(!game.evict_scene(first));
    assert_eq!(game.trees().collect::<Vec<_>>(), [tree(TreeKind::Bare), tree(TreeKind::Broadleaf)]);
    assert!(observed(&game).1.contains(&format!("\"{second}/0\"")));
    assert!(observed(&game).1.contains(&format!("\"{third}/0\"")));
    assert_eq!(game.tree_state(third, 0), Some(TreeState::Standing));
    assert_eq!(game.joint_presentations(arpg_sim::Alpha::ONE).count(), 2);
    game.restart().unwrap();
    assert_eq!(observed(&game), fresh);
    game.start_scene(&GameScene::default()).unwrap();
    assert_eq!(game.trees().count(), 0);
    assert_eq!(game.joint_presentations(arpg_sim::Alpha::ONE).count(), 0);
}
#[test]
fn excess_trees_preserve_pending_work_live_state_and_restart() {
    let authored = scene(TreeKind::Pine, MAX_TREES);
    let mut game = Game::from_scene(&authored).unwrap();
    let mut control = Game::from_scene(&authored).unwrap();
    for game in [&mut game, &mut control] {
        assert!(game.request_spawn(Vec2::new(4.0, 0.0), ActorDefinition::passive_hostile()).is_some());
    }
    let before = observed(&game);
    assert!(game.load_scene(&scene(TreeKind::Bare, 1)).is_err());
    assert!(game.start_scene(&scene(TreeKind::Bare, MAX_TREES + 1)).is_err());
    assert_eq!(observed(&game), before);
    step(&mut game);
    step(&mut control);
    assert_eq!(observed(&game), observed(&control));
    assert_eq!(game.enemy_count(), 1);
    game.restart().unwrap();
    assert_eq!(observed(&game), observed(&Game::from_scene(&authored).unwrap()));
}
#[test]
fn trunk_geometry_affects_physics_hash_and_validates_at_construction() {
    let a = Game::from_scene(&scene(TreeKind::Pine, 1)).unwrap();
    let b = Game::from_scene(&scene(TreeKind::Bare, 1)).unwrap();
    assert_ne!(a.engine_hash(), b.engine_hash());
    assert_ne!(a.hash(), b.hash());
    for (position, yaw, scale) in [(Vec2::splat(f32::NAN), 0.0, 1.0), (Vec2::splat(100.0), 0.0, 1.0),
        (Vec2::ZERO, f32::INFINITY, 1.0), (Vec2::ZERO, 0.0, 0.0), (Vec2::ZERO, 0.0, 2.01), (Vec2::new(96.0, 0.0), 0.0, 1.0)] {
        assert!(TreeSpec::new(TreeKind::Pine, position, yaw, scale).is_err());
    }
}

#[test]
fn trunks_leave_wall_capacity_and_authored_body_identities_alone() {
    let mut authored = GameScene { trees: vec![tree(TreeKind::Pine)], ..GameScene::default() };
    authored.engine.walls = vec![arpg_sim::Wall::new(Vec2::new(20.0, 20.0), Vec2::ONE, 0.0).unwrap(); arpg_sim::MAX_WALLS];
    let bare = Game::from_scene(&GameScene { trees: Vec::new(), ..authored.clone() }).unwrap();
    authored.engine.bodies.push(arpg_game::Placed { pos: (4.0, 0.0), what: ActorDefinition::passive_hostile() });
    let game = Game::from_scene(&authored).unwrap();
    assert_eq!(game.walls().count(), arpg_sim::MAX_WALLS);
    let owner = game.scene_instances().next().unwrap().0;
    let bodies = game.scene_bodies(owner).unwrap();
    assert_eq!(bodies.len(), 2, "the authored body, then the trunk");
    assert_eq!(game.tree_body(owner, 0), Some(bodies[1]));
    assert!(game.health(bodies[0]).is_some() && game.health(bodies[1]).is_none());
    assert!(game.joint(bodies[0]).is_none());
    assert_ne!(bare.hash(), game.hash());
}

#[test]
fn scaled_rotated_trunks_share_motion_and_attack_obstruction_without_extra_wall_meshes() {
    let tree = TreeSpec::new(TreeKind::Broadleaf, Vec2::new(0.0, 2.0), std::f32::consts::FRAC_PI_4, 1.5).unwrap();
    let mut authored = GameScene { trees: vec![tree], ..GameScene::default() };
    authored.engine.walls.push(arpg_sim::Wall::new(Vec2::new(20.0, 20.0), Vec2::ONE, 0.0).unwrap());
    let mut game = Game::from_scene(&authored).unwrap();
    let owner = game.walls().next().unwrap().0;
    assert_eq!(game.walls().count(), 1);
    let trunk = game.joint(game.tree_body(owner, 0).unwrap()).unwrap().collider();
    assert!(trunk.size().abs_diff_eq(Vec2::splat(0.72), 0.00001));
    let near_face = 2.0 - 0.36 * 2.0_f32.sqrt();
    assert!((game.solid_hit(Vec2::ZERO, Vec2::new(0.0, 4.0)).unwrap().unwrap().point().y - near_face).abs() < 0.00001);
    game.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true));
    assert!((game.beam().unwrap().end().y - near_face).abs() < 0.00001);
    // A launched body must stop at the trunk rather than crossing it in one
    // step; its 80 kg m/s stays below the 495 kg m/s root of this broadleaf.
    let body = game.place(Vec2::new(0.0, 1.0), ActorDefinition::passive_hostile()).unwrap();
    assert!(game.apply_impulse(body, arpg_sim::Impulse::try_from((0.0, 80.0)).unwrap()));
    for _ in 0..12 {
        step(&mut game);
        assert!(game.body_pos(body).unwrap().z < near_face);
    }
    assert_eq!(game.tree_state(owner, 0), Some(TreeState::Standing));
    assert!(game.evict_scene(owner));
    assert!(game.solid_hit(Vec2::ZERO, Vec2::new(0.0, 4.0)).unwrap().is_none());
}
