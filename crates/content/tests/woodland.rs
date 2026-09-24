//! Authored woodland uses the game-owned validation boundary.
#[test]
fn tree_files_reject_invalid_fields_and_placements() {
    for raw in ["(kind: Pine, position: (100.0, 0.0), yaw: 0.0, scale: 1.0)",
        "(kind: Pine, position: (0.0, 0.0), yaw: 0.0, scale: -1.0)",
        "(kind: Pine, position: (0.0, 0.0), yaw: 0.0, scale: 1.0, collision: true)"] {
        assert!(ron::from_str::<arpg_game::GameScene>(&format!("(engine: (name: \"invalid\"), trees: [{raw}])")).is_err());
    }
    let scene = arpg_content::load_scene(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes/soul_trial.ron")).unwrap();
    assert_eq!(scene.trees.len(), 96);
    assert!(scene.trees.iter().all(|t| t.position().length() > 4.2));
    assert!(scene.trees.iter().any(|t| t.position().length() < 9.0), "scatter enters the fighting area");
    for sign_x in [-1.0, 1.0] {
        for sign_z in [-1.0, 1.0] {
            assert!(scene.trees.iter().any(|t| t.position().x * sign_x > 40.0 && t.position().y * sign_z > 40.0));
        }
    }
    for (i, tree) in scene.trees.iter().enumerate() {
        assert!(scene.trees[..i].iter().all(|other| tree.position().distance(other.position()) >= 3.5));
        assert!(scene.engine.sources.iter().all(|source| (tree.position().x - source.pos.0).hypot(tree.position().y - source.pos.1) >= 2.0));
    }
    let mut game = arpg_game::Game::from_scene(&scene).unwrap();
    assert_eq!(game.walls().count(), scene.trees.len());
    assert_eq!(game.visible_walls().count(), 0);
    let hash = game.hash();
    game.restart().unwrap();
    assert_eq!(game.hash(), hash);
}
