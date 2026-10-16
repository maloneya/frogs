//! The standalone staff is admitted through the production static boundary.
use arpg_assets::import_glb;
use glam::Vec3;

#[test]
fn traveler_staff_preserves_scale_grip_and_blade_axis() {
    check_staff(include_bytes!("../../../assets/weapons/thornbound-staff/variants/large-talon/thornbound-staff.glb"), 2.34, 0.269);
    check_staff(include_bytes!("../../../assets/weapons/thornbound-staff/variants/sword-glaive/thornbound-staff.glb"), 2.53, 0.047);
}

#[test]
fn sword_glaive_has_matching_edges_and_a_centered_point() {
    let mesh = import_glb(include_bytes!("../../../assets/weapons/thornbound-staff/variants/sword-glaive/thornbound-staff.glb")).unwrap();
    let edges: Vec<_> = mesh.vertices().iter().map(|v| v.position())
        .filter(|p| p.y > 0.87 && p.z.abs() < 1e-6).collect();
    assert!(edges.len() >= 80, "both cutting edges must be present");
    for p in &edges {
        assert!(edges.iter().any(|q| (q.y - p.y).abs() < 1e-5 && (q.x + p.x - 0.012).abs() < 1e-5),
            "a sword edge must have its mirrored partner: {p:?}");
    }
    let point = edges.iter().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    assert!((point.x - 0.006).abs() < 0.001 && (point.y - 1.505).abs() < 1e-5);
}

fn check_staff(bytes: &[u8], height: f32, max_x: f32) {
    let mesh = import_glb(bytes).expect("the weapon must cross the runtime importer");
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for vertex in mesh.vertices() {
        min = min.min(vertex.position());
        max = max.max(vertex.position());
    }
    assert!((max.y - min.y - height).abs() < 0.002, "metre scale: {min:?}..{max:?}");
    assert!((max.x - max_x).abs() < 0.002, "the authored lateral extent is preserved");
    assert!(max.z - min.z < 0.09, "the blade and shaft remain slender");
    let grip: Vec<_> = mesh.vertices().iter().map(|v| v.position())
        .filter(|p| p.y.abs() < 0.17).collect();
    assert!(!grip.is_empty());
    assert!(grip.iter().all(|p| (0.021..0.024).contains(&p.x.hypot(p.z))),
        "grasp fitting relies on an uninterrupted cylindrical grip");
    assert_eq!((mesh.base_color_texture().width(), mesh.base_color_texture().height()), (2048, 1024));
    assert_eq!(mesh.base_color_texture().mip_level_count(), 12);
    assert!(mesh.index_count() / 3 < 25_000);
}

#[test]
fn retained_staffs_keep_equipped_grips_and_casting_tips() {
    for bytes in [
        include_bytes!("../../../assets/weapons/thornbound-staff/equipped/large-talon/player.glb").as_slice(),
        include_bytes!("../../../assets/weapons/thornbound-staff/equipped/sword-glaive/player.glb").as_slice(),
    ] {
        let asset = arpg_assets::import_character_glb(bytes).unwrap();
        assert_eq!((asset.joint_count(), asset.clip_count()), (54, 8));
        assert_eq!((asset.parts().len(), asset.materials().len(), asset.images().len()), (3, 3, 3));
        let idle = asset.clip_named("Idle").unwrap();
        for (parent, child) in [("RightHand", "RightGrip"), ("RightGrip", "Weapon"), ("Weapon", "BeamOrigin")] {
            asset.validate_rigid_attachment(parent, child, idle).unwrap();
        }
        let mut pose = asset.bind_pose();
        let matrix = |pose: &arpg_assets::CharacterPose, name| {
            asset.joint_transform(pose, asset.joint_named(name).unwrap()).unwrap()
        };
        let tip = matrix(&pose, "BeamOrigin").transform_point3(Vec3::ZERO);
        let staff = asset.parts().iter().find(|part| part.node_name() == "ThornboundStaff").unwrap();
        assert!(asset.vertices()[staff.vertices()].iter().any(|vertex| vertex.position().distance(tip) < 0.001));
        for name in ["Idle", "Run", "RunBackward", "RunLeft", "RunRight", "AttackCleave", "AttackSlam", "Disintegrate"] {
            let clip = asset.clip_named(name).unwrap();
            for fraction in [0.0, 0.4, 0.5, 0.6, 1.0] {
                assert!(asset.sample(clip, asset.clip(clip).unwrap().duration_seconds() * fraction, &mut pose));
                let weapon = matrix(&pose, "Weapon");
                let grip = weapon.transform_point3(Vec3::ZERO);
                assert!(grip.distance(matrix(&pose, "RightGrip").transform_point3(Vec3::ZERO)) < 0.001, "{name}");
                if name == "AttackSlam" && (0.4..=0.6).contains(&fraction) {
                    let free_hand = matrix(&pose, "LeftGrip").transform_point3(Vec3::ZERO) - grip;
                    let axis = weapon.transform_vector3(Vec3::Y).normalize();
                    assert!((free_hand - axis * free_hand.dot(axis)).length() > 0.25,
                        "the free hand must stay visibly clear of the staff during the one-handed slam");
                }
            }
        }
    }
}
