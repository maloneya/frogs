//! Authored woodland uses the game-owned validation boundary.
#[test]
fn tree_files_reject_invalid_fields_and_placements() {
    for raw in ["(kind: Pine, position: (100.0, 0.0), yaw: 0.0, scale: 1.0)",
        "(kind: Pine, position: (0.0, 0.0), yaw: 0.0, scale: -1.0)",
        "(kind: Pine, position: (0.0, 0.0), yaw: 0.0, scale: 1.0, collision: true)"] {
        assert!(ron::from_str::<arpg_game::GameScene>(&format!("(engine: (name: \"invalid\"), trees: [{raw}])")).is_err());
    }
}
