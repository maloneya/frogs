//! Benchmark admission checks cover the feature combination before measurement.

#[test]
fn firelit_workloads_have_the_same_crowd_and_blockers_with_weather_as_the_difference() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let night = arpg_content::load_scene(&root.join("benchmarks/night_fire.ron")).unwrap();
    let storm = arpg_content::load_scene(&root.join("benchmarks/storm_fire.ron")).unwrap();
    assert!(!night.environment.storm());
    assert!(storm.environment.storm());
    let mut without_weather = storm.clone();
    without_weather.environment = without_weather.environment.with_storm(false);
    assert_eq!(serde_json::to_value(&night).unwrap(), serde_json::to_value(&without_weather).unwrap(),
        "only weather differs in the controlled pair");
    for scene in [&night, &storm] {
        let game = arpg_game::Game::from_scene(scene).unwrap();
        assert_eq!(game.enemy_count(), 10_000);
        assert_eq!(game.torches().count(), 1);
        assert_eq!(game.trees().count(), 4);
        assert!(game.environment_light().directional_intensity() > 0.0);
        assert!(game.all_positions_finite());
    }
}
