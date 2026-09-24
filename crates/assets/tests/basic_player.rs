//! Contract test for the Blender-authored basic player asset.

use arpg_assets::import_character_glb;

const BASIC_PLAYER: &[u8] =
    include_bytes!("../../../assets/characters/basic-player/basic-player.glb");

#[test]
fn basic_player_satisfies_the_player_presentation_contract() {
    let character = import_character_glb(BASIC_PLAYER).unwrap();

    assert!(character.vertex_count() > 300);
    assert!(character.index_count() > 500);
    assert_eq!(character.images()[0].width(), 64);
    assert_eq!(character.images()[0].height(), 64);
    assert_eq!(character.parts().len(), 1);
    assert_eq!(character.materials().len(), 1);
    assert_eq!(character.images().len(), 1);
    assert_eq!(character.joint_count(), 9);
    assert_eq!(character.clip_count(), 8);
    assert!(character.joint_named("Weapon").is_some());
    assert!(character.joint_named("BeamOrigin").is_some());

    for name in [
        "Idle",
        "Run",
        "RunBackward",
        "RunLeft",
        "RunRight",
        "AttackCleave",
        "AttackSlam",
        "Disintegrate",
    ] {
        assert!(character.clip_named(name).is_some(), "missing {name}");
    }
}

#[test]
fn masked_attacks_keep_running_legs_and_refuse_foreign_state_atomically() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let other = import_character_glb(BASIC_PLAYER).unwrap();
    let mut pose = asset.bind_pose();
    let mut attack = asset.bind_pose();
    asset.sample(asset.clip_named("RunBackward").unwrap(), 0.0, &mut pose);
    asset.sample(asset.clip_named("AttackCleave").unwrap(), 0.4, &mut attack);
    let legs = ["LeftLeg", "RightLeg"].map(|name| asset.joint_named(name).unwrap());
    let before = legs.map(|joint| asset.joint_transform(&pose, joint).unwrap());
    let mask = asset.pose_mask(&[("Spine", 1.0), ("Head", 1.0), ("LeftArm", 1.0), ("RightArm", 1.0), ("Weapon", 1.0)]).unwrap();
    assert!(asset.blend(&mut pose, &attack, &mask, 1.0));
    for (joint, before) in legs.into_iter().zip(before) {
        assert_eq!(asset.joint_transform(&pose, joint).unwrap(), before);
    }
    let weapon = asset.joint_named("Weapon").unwrap();
    assert!(asset.joint_transform(&pose, weapon).unwrap().abs_diff_eq(asset.joint_transform(&attack, weapon).unwrap(), 1e-5));
    let before: Vec<_> = pose.joint_matrices().iter().map(|m| m.matrix()).collect();
    assert!(!asset.blend(&mut pose, &other.bind_pose(), &mask, 1.0));
    assert!(!asset.blend(&mut pose, &attack, &other.full_mask(), 1.0));
    for invalid in [f32::NAN, -0.1, 1.1] { assert!(!asset.blend(&mut pose, &attack, &mask, invalid)); }
    assert_eq!(before, pose.joint_matrices().iter().map(|m| m.matrix()).collect::<Vec<_>>());
    assert!(asset.pose_mask(&[("missing", 1.0)]).is_err());
    assert!(asset.pose_mask(&[("Spine", f32::NAN)]).is_err());
    assert!(asset.pose_mask(&[("Spine", 1.0), ("Spine", 0.0)]).is_err());
}
