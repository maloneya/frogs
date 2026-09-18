//! Both static chest poses satisfy the runtime asset and metre-scale contract.
use arpg_assets::import_glb;

#[test]
fn closed_and_open_chests_share_footprint_and_raise_the_lid() {
    let bounds = |bytes: &[u8]| {
        let mesh = import_glb(bytes).unwrap();
        assert_eq!(mesh.base_color_texture().width(), 8);
        assert_eq!(mesh.base_color_texture().height(), 8);
        assert!(mesh.vertex_count() < 1_000);
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        for vertex in mesh.vertices() { min = min.min(vertex.position()); max = max.max(vertex.position()); }
        (min, max)
    };
    let (closed_min, closed_max) = bounds(include_bytes!("../../../assets/world/chest/closed.glb"));
    let (open_min, open_max) = bounds(include_bytes!("../../../assets/world/chest/open.glb"));
    assert!(closed_min.y.abs() < 1e-5 && open_min.y.abs() < 1e-5);
    assert!((closed_max.x - closed_min.x - 1.04).abs() < 1e-5);
    assert!((open_max.x - open_min.x - 1.04).abs() < 1e-5);
    assert!(closed_max.y < 0.65);
    assert!(open_max.y > 1.0 && open_max.y < 1.2);
}
