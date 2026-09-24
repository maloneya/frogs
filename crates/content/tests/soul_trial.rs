//! The authored trial starts with every spawn area inside its expanded central light.
#[test]
fn all_spawn_areas_surround_the_player_within_the_central_light() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes/soul_trial.ron");
    let scene = arpg_content::load_scene(&path).unwrap();
    let game = arpg_game::Game::from_scene(&scene).unwrap();
    assert_eq!(game.torch_count(), 1);
    let light = game.torches().next().unwrap().light();
    assert_eq!(light.position().x, 0.0);
    assert_eq!(light.position().z, 0.0);
    assert_eq!(scene.engine.sources.len(), 8);
    let radius = scene.engine.sources[0].pos.0.hypot(scene.engine.sources[0].pos.1);
    let mut sum = (0.0, 0.0);
    for (i, source) in scene.engine.sources.iter().enumerate() {
        let distance = source.pos.0.hypot(source.pos.1);
        assert!((distance - radius).abs() < 0.01, "one circle around the player");
        assert!((distance + source.radius).hypot(light.position().y) < light.range(), "the expanded light reaches the whole spawn area");
        assert!(!source.enabled);
        assert!(scene.engine.sources[..i].iter().all(|other| other.pos != source.pos));
        sum.0 += source.pos.0;
        sum.1 += source.pos.1;
    }
    assert!(sum.0.abs() < 0.01 && sum.1.abs() < 0.01, "balanced around the origin");
    assert_eq!(scene.chests.len(), 1);
    assert!(scene.doors.is_empty());
}
