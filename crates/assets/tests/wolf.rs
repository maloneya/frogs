//! Contract test for the Blender-authored wolf horde asset.

use arpg_assets::import_character_glb;

const WOLF: &[u8] =
    include_bytes!("../../../assets/characters/wolf/wolf.glb");
const SHADOW: &[u8] = include_bytes!("../../../assets/characters/wolf/wolf.shadow.glb");

#[test]
fn coarse_shadow_shares_the_visible_palette_without_changing_visible_geometry() {
    let mut visible = import_character_glb(WOLF).unwrap();
    let original = bytemuck::cast_slice::<_, u8>(visible.vertices()).to_vec();
    let indices = visible.indices().to_vec();
    let proxy = import_character_glb(SHADOW).unwrap();
    for role in ["Idle", "Walk", "Run", "Bite", "Leap", "Hit", "Death"] {
        let mut a = visible.bind_pose();
        let mut b = proxy.bind_pose();
        for seconds in [0.0, 0.125, 0.25, 0.375, 0.5] {
            visible.sample(visible.clip_named(role).unwrap(), seconds, &mut a);
            proxy.sample(proxy.clip_named(role).unwrap(), seconds, &mut b);
            for (a, b) in a.joint_matrices().iter().zip(b.joint_matrices()) {
                assert!(a.matrix().abs_diff_eq(b.matrix(), 0.00001));
            }
        }
    }
    visible.attach_shadow(proxy).unwrap();
    let shadow = visible.shadow_mesh().unwrap();
    assert!(shadow.vertices().len() < visible.vertex_count());
    assert!(shadow.indices().len() < visible.index_count());
    assert_eq!(original, bytemuck::cast_slice::<_, u8>(visible.vertices()));
    assert_eq!(indices, visible.indices());
    let mut pose = visible.bind_pose();
    visible.sample(visible.clip_named("Run").unwrap(), 0.125, &mut pose);
    for vertex in shadow.vertices() {
        let p = vertex.joints().into_iter().zip(vertex.weights()).fold(glam::Vec3::ZERO, |sum, (joint, weight)|
            sum + pose.joint_matrices()[usize::from(joint)].matrix().transform_point3(vertex.position()) * weight);
        assert!(p.is_finite());
    }
}

#[test]
fn wolf_satisfies_the_horde_presentation_contract() {
    let character = import_character_glb(WOLF).unwrap();

    assert!(character.vertex_count() > 250);
    assert!(character.index_count() > 400);
    assert_eq!(character.images()[0].width(), 128);
    assert_eq!(character.images()[0].height(), 256);
    assert_eq!(character.joint_count(), 33);
    assert!(character.clip_named("Idle").is_some());
    assert!(character.clip_named("Run").is_some());
    assert!(character.clip_named("Hit").is_some());
    assert!(character.clip_named("Death").is_some());
}

#[test]
fn wolf_has_four_supported_limbs_forward_anatomy_and_distinct_looping_gaits() {
    let asset = import_character_glb(WOLF).unwrap();
    let mut pose = asset.bind_pose();
    let point = |pose: &arpg_assets::CharacterPose, name: &str| asset.joint_transform(pose,
        asset.joint_named(name).unwrap()).unwrap().transform_point3(glam::Vec3::ZERO);
    let mut movements = Vec::new();
    for name in ["Idle", "Walk", "Run"] {
        let clip = asset.clip_named(name).unwrap();
        let duration = asset.clip(clip).unwrap().duration_seconds();
        asset.sample(clip, 0.0, &mut pose);
        let opening = pose.joint_matrices().to_vec();
        let paw = point(&pose, "LeftForeToe");
        asset.sample(clip, duration * 0.25, &mut pose);
        movements.push(paw.distance(point(&pose, "LeftForeToe")));
        asset.sample(clip, duration, &mut pose);
        for (start, end) in opening.iter().zip(pose.joint_matrices()) {
            assert!(start.matrix().abs_diff_eq(end.matrix(), 1e-4), "{name} loop seam");
        }
    }
    assert!(movements[0] < 0.02, "idle must retain planted support");
    assert!(movements[1] > 0.1 && movements[2] > 0.1, "walk/run need paw travel: {movements:?}");
    assert!(asset.clip(asset.clip_named("Run").unwrap()).unwrap().duration_seconds()
        < asset.clip(asset.clip_named("Walk").unwrap()).unwrap().duration_seconds());
    let bind = asset.bind_pose();
    for side in ["Left", "Right"] {
        for end in ["ForeToe", "HindToe"] {
            let paw = point(&bind, &format!("{side}{end}"));
            assert!((0.0..0.06).contains(&paw.y), "ground support {side}{end}: {paw:?}");
            assert_eq!(paw.x > 0.0, side == "Left");
        }
    }
    assert!(point(&bind, "Head").z > point(&bind, "Pelvis").z + 1.0, "+Z is the muzzle direction");
    for role in ["Bite", "Leap", "Hit", "Death"] { assert!(asset.clip_named(role).is_some()); }
    assert_eq!(asset.clip_count(), 7);
}

#[test]
fn bite_closes_the_jaw_during_contact_and_attacks_recover_to_idle() {
    let asset = import_character_glb(WOLF).unwrap();
    let mut pose = asset.bind_pose();
    let relative_jaw = |pose: &arpg_assets::CharacterPose| {
        let head = asset.joint_transform(pose, asset.joint_named("Head").unwrap()).unwrap();
        let jaw = asset.joint_transform(pose, asset.joint_named("Jaw").unwrap()).unwrap();
        (head.inverse() * jaw).to_scale_rotation_translation().1
    };
    let bite = asset.clip_named("Bite").unwrap();
    let duration = asset.clip(bite).unwrap().duration_seconds();
    asset.sample(bite, duration * 0.4, &mut pose);
    let opened = relative_jaw(&pose);
    asset.sample(bite, duration * 0.5, &mut pose);
    assert!(opened.angle_between(relative_jaw(&pose)) > 0.3, "jaw must shut during active contact");
    asset.sample(asset.clip_named("Idle").unwrap(), 0.0, &mut pose);
    let idle = pose.joint_matrices().to_vec();
    for role in ["Bite", "Leap"] {
        let clip = asset.clip_named(role).unwrap();
        for time in [0.0, asset.clip(clip).unwrap().duration_seconds()] {
            asset.sample(clip, time, &mut pose);
            for (ready, current) in idle.iter().zip(pose.joint_matrices()) {
                assert!(ready.matrix().abs_diff_eq(current.matrix(), 0.002), "{role} must meet idle at {time}");
            }
        }
    }
}
