//! Contract test for the Blender-authored basic player asset.

use arpg_assets::import_character_glb;

const BASIC_PLAYER: &[u8] =
    include_bytes!("../../../assets/characters/basic-player/basic-player.glb");

#[test]
fn basic_player_satisfies_the_player_presentation_contract() {
    let character = import_character_glb(BASIC_PLAYER).unwrap();

    assert!(character.vertex_count() > 300);
    assert!(character.index_count() > 500);
    assert_eq!(character.images()[0].width(), 256);
    assert_eq!(character.images()[0].height(), 256);
    assert_eq!(character.parts().len(), 2);
    assert_eq!(character.materials().len(), 2);
    assert_eq!(character.images().len(), 2);
    assert!(character.parts().iter().any(|part| part.node_name() == "HoodedCloak"));
    assert_eq!(character.joint_count(), 54);
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
fn longsword_fits_human_scale_and_keeps_the_beam_socket_on_its_tip() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let weapon = asset.joint_named("Weapon").unwrap();
    let bind = asset.bind_pose();
    let grip = asset.joint_transform(&bind, weapon).unwrap().transform_point3(glam::Vec3::ZERO);
    let socket = asset.joint_named("BeamOrigin").unwrap();
    let tip = asset.joint_transform(&bind, socket).unwrap().transform_point3(glam::Vec3::ZERO);
    let axis = (tip - grip).normalize();
    let weapon_index = asset.joint_index(weapon).unwrap();
    let sword: Vec<_> = asset.vertices().iter().filter(|vertex| {
        vertex.joints().into_iter().zip(vertex.weights())
            .any(|(joint, weight)| usize::from(joint) == weapon_index && weight > 0.99)
    }).map(|vertex| vertex.position()).collect();
    assert!(!sword.is_empty());
    let along = |point: glam::Vec3| (point - grip).dot(axis);
    let front = sword.iter().copied().map(along).fold(f32::NEG_INFINITY, f32::max);
    let back = sword.iter().copied().map(along).fold(f32::INFINITY, f32::min);
    assert!((1.2..1.5).contains(&(front - back)), "longsword is out of proportion to the two-metre body");
    let radial = |point: glam::Vec3| { let d = point - grip; (d - axis * d.dot(axis)).length() };
    assert!(sword.iter().all(|point| along(*point) < 0.15 || radial(*point) < 0.035),
        "blade is too broad for the character");
    assert!(sword.iter().all(|point| radial(*point) < 0.15), "guard is oversized");
    assert!(sword.iter().any(|point| point.distance(tip) < 1e-4), "beam socket left the physical tip");
    assert!((front - grip.distance(tip)).abs() < 1e-4, "geometry extends beyond the beam socket");
}

#[test]
fn accepted_body_keeps_human_scale_and_a_narrow_shoulder_span() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let bind = asset.bind_pose();
    let point = |name| asset.joint_transform(&bind, asset.joint_named(name).unwrap())
        .unwrap().transform_point3(glam::Vec3::ZERO);
    let shoulders = point("LeftArm").distance(point("RightArm"));
    assert!((0.43..0.48).contains(&shoulders), "bind rig regained the oversized shoulders: {shoulders}");
    let body = asset.parts().iter().find(|part| part.node_name() == "Swordsman").unwrap();
    let body_vertices = &asset.vertices()[body.vertices()];
    let top = body_vertices.iter().map(|v| v.position().y).fold(f32::NEG_INFINITY, f32::max);
    let bottom = body_vertices.iter().map(|v| v.position().y).fold(f32::INFINITY, f32::min);
    assert!((top - bottom - 2.0).abs() < 1e-4, "body lost its uniform two-metre scale");
    let chest = asset.joint_index(asset.joint_named("Chest").unwrap()).unwrap();
    let rib_cage: Vec<_> = body_vertices.iter().filter(|v| {
        v.joints().into_iter().zip(v.weights()).any(|(joint, weight)| usize::from(joint) == chest && weight > 0.9)
    }).collect();
    assert!(rib_cage.len() > 50);
    assert!(rib_cage.iter().all(|v| v.position().x.abs() < 0.25), "chest surface regained the oversized barrel shape");
}

#[test]
fn cloak_has_a_raised_open_hood_and_a_separate_draped_hem() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let part = asset.parts().iter().find(|part| part.node_name() == "HoodedCloak").unwrap();
    let cloth = &asset.vertices()[part.vertices()];
    let head = asset.joint_index(asset.joint_named("Head").unwrap()).unwrap();
    let hood: Vec<_> = cloth.iter().filter(|v| {
        v.joints().into_iter().zip(v.weights()).any(|(joint, weight)| usize::from(joint) == head && weight > 0.99)
    }).map(|v| v.position()).collect();
    assert!(hood.len() > 100, "raised hood geometry is absent");
    assert!(hood.iter().any(|p| p.y > 2.04), "hood must cover the crown");
    assert!(hood.iter().any(|p| p.y > 1.8 && p.z < -0.1), "hood must cover the back of the head");
    assert!(!hood.iter().any(|p| (1.8..1.9).contains(&p.y) && p.x.abs() < 0.10 && p.z > 0.15),
        "hood closed the face opening");
    let hem: Vec<_> = cloth.iter().filter(|v| (0.2..0.3).contains(&v.position().y))
        .map(|v| v.position()).collect();
    assert!(hem.iter().any(|p| p.x > 0.40) && hem.iter().any(|p| p.x < -0.40),
        "cloak lost its full draped hem");
    assert!(hem.iter().any(|p| p.z < -0.25), "cloak no longer hangs behind the legs");
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
    let mask = asset.pose_mask(&[("Spine", 1.0), ("Chest", 1.0), ("Neck", 1.0), ("Head", 1.0), ("LeftShoulder", 1.0), ("RightShoulder", 1.0), ("LeftArm", 1.0), ("RightArm", 1.0), ("LeftForearm", 1.0), ("RightForearm", 1.0), ("LeftHand", 1.0), ("RightHand", 1.0), ("LeftIndex1", 1.0), ("RightIndex1", 1.0), ("LeftThumb1", 1.0), ("RightThumb1", 1.0), ("RightGrip", 1.0), ("LeftGrip", 1.0), ("Weapon", 1.0)]).unwrap();
    assert!(asset.blend(&mut pose, &attack, &mask, 1.0));
    for (joint, before) in legs.into_iter().zip(before) {
        assert_eq!(asset.joint_transform(&pose, joint).unwrap(), before);
    }
    let weapon = asset.joint_named("Weapon").unwrap();
    let pelvis = asset.joint_named("Pelvis").unwrap();
    let relative = |pose: &arpg_assets::CharacterPose| asset.joint_transform(pose, pelvis).unwrap().inverse()
        * asset.joint_transform(pose, weapon).unwrap();
    assert!(relative(&pose).abs_diff_eq(relative(&attack), 1e-5));
    let before: Vec<_> = pose.joint_matrices().iter().map(|m| m.matrix()).collect();
    assert!(!asset.blend(&mut pose, &other.bind_pose(), &mask, 1.0));
    assert!(!asset.blend(&mut pose, &attack, &other.full_mask(), 1.0));
    for invalid in [f32::NAN, -0.1, 1.1] { assert!(!asset.blend(&mut pose, &attack, &mask, invalid)); }
    assert_eq!(before, pose.joint_matrices().iter().map(|m| m.matrix()).collect::<Vec<_>>());
    assert!(asset.pose_mask(&[("missing", 1.0)]).is_err());
    assert!(asset.pose_mask(&[("Spine", f32::NAN)]).is_err());
    assert!(asset.pose_mask(&[("Spine", 1.0), ("Spine", 0.0)]).is_err());
}

#[test]
fn parent_aligned_layers_preserve_aim_and_reject_foreign_poses_without_mutation() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let foreign = import_character_glb(BASIC_PLAYER).unwrap();
    let names = ["Spine", "Chest", "Neck", "Head", "LeftShoulder", "LeftArm", "LeftForearm", "LeftHand",
        "LeftIndex1", "LeftThumb1", "RightShoulder", "RightArm", "RightForearm", "RightHand", "RightIndex1", "RightThumb1", "RightGrip", "LeftGrip", "Weapon", "BeamOrigin"];
    let mask = asset.pose_mask_parent_aligned(&names.map(|name| (name, 1.0))).unwrap();
    let mut base = asset.bind_pose();
    let mut overlay = asset.bind_pose();
    asset.sample(asset.clip_named("RunLeft").unwrap(), 0.1, &mut base);
    asset.sample(asset.clip_named("AttackCleave").unwrap(), 0.4, &mut overlay);
    let pelvis = asset.joint_named("Pelvis").unwrap();
    let legs = ["LeftLeg", "RightLeg", "LeftShin", "RightShin"].map(|n| asset.joint_named(n).unwrap());
    let before = legs.map(|joint| asset.joint_transform(&base, joint).unwrap());
    let hips = asset.joint_transform(&base, pelvis).unwrap();
    assert!(asset.blend_parent_aligned(&mut base, &overlay, &mask, 1.0));
    assert_eq!(asset.joint_transform(&base, pelvis).unwrap(), hips);
    for (joint, transform) in legs.into_iter().zip(before) {
        assert_eq!(asset.joint_transform(&base, joint).unwrap(), transform);
    }
    for name in ["Spine", "Chest", "RightHand", "Weapon"] {
        let joint = asset.joint_named(name).unwrap();
        let rotation = |pose| asset.joint_transform(pose, joint).unwrap().to_scale_rotation_translation().1;
        assert!(rotation(&base).abs_diff_eq(rotation(&overlay), 1e-5), "{name} aim follows the hips");
    }
    let before: Vec<_> = base.joint_matrices().iter().map(|m| m.matrix()).collect();
    for weight in [f32::NAN, -0.1, 1.1] {
        assert!(!asset.blend_parent_aligned(&mut base, &overlay, &mask, weight));
    }
    assert!(!asset.blend_parent_aligned(&mut base, &foreign.bind_pose(), &mask, 1.0));
    assert!(!asset.blend_parent_aligned(&mut base, &overlay, &foreign.full_mask(), 1.0));
    let reentered = asset.pose_mask(&[("Pelvis", 1.0), ("Chest", 1.0)]).unwrap();
    assert!(asset.pose_mask_parent_aligned(&[("Pelvis", 1.0), ("Chest", 1.0)]).is_err());
    assert!(!asset.blend_parent_aligned(&mut base, &overlay, &reentered, 1.0));
    assert_eq!(before, base.joint_matrices().iter().map(|m| m.matrix()).collect::<Vec<_>>());
}

#[test]
fn locomotion_swings_both_arms_turns_strafe_hips_and_keeps_loop_and_grip_continuity() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let bind = asset.bind_pose();
    for name in ["Run", "RunBackward", "RunLeft", "RunRight"] {
        let clip = asset.clip_named(name).unwrap();
        let duration = asset.clip(clip).unwrap().duration_seconds();
        let mut pose = asset.bind_pose();
        let point = |pose: &arpg_assets::CharacterPose, name| asset.joint_transform(pose,
            asset.joint_named(name).unwrap()).unwrap().transform_point3(glam::Vec3::ZERO);
        let mut hands = [Vec::new(), Vec::new()];
        let mut grip = None;
        for i in 0..=120 {
            asset.sample(clip, duration * i as f32 / 120.0, &mut pose);
            for (hand, joint) in hands.iter_mut().zip(["LeftHand", "RightHand"]) { hand.push(point(&pose, joint)); }
            let relative = asset.joint_transform(&pose, asset.joint_named("RightHand").unwrap()).unwrap().inverse()
                * asset.joint_transform(&pose, asset.joint_named("Weapon").unwrap()).unwrap();
            if let Some(previous) = grip { assert!(relative.abs_diff_eq(previous, 1e-4), "{name} grip moved"); }
            else { grip = Some(relative); }
        }
        for hand in hands {
            assert!(hand[0].abs_diff_eq(*hand.last().unwrap(), 1e-5), "{name} loop seam");
            let span = hand.iter().flat_map(|a| hand.iter().map(move |b| a.distance(*b))).fold(0.0_f32, f32::max);
            assert!(span > 0.25, "{name} arm froze: {span}");
        }
        asset.sample(clip, 0.0, &mut pose);
        let forward = |name| {
            let joint = asset.joint_named(name).unwrap();
            (asset.joint_transform(&pose, joint).unwrap() * asset.joint_transform(&bind, joint).unwrap().inverse())
                .transform_vector3(glam::Vec3::Z).normalize()
        };
        if name == "RunLeft" { assert!(forward("Pelvis").x < -0.7); }
        if name == "RunRight" { assert!(forward("Pelvis").x > 0.7); }
        assert!(forward("Chest").x.abs() < 0.3, "{name} torso followed the sideways hips");
    }
}

#[test]
fn articulated_limbs_deform_and_attacks_hold_then_release() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    assert!(asset.vertices().iter().any(|v| v.weights().into_iter().filter(|w| *w > 0.0).count() > 1));
    let mut pose = asset.bind_pose();
    let shin = asset.joint_named("RightShin").unwrap();
    let bind = asset.joint_transform(&pose, shin).unwrap();
    asset.sample(asset.clip_named("Run").unwrap(), 0.0, &mut pose);
    assert!(!bind.abs_diff_eq(asset.joint_transform(&pose, shin).unwrap(), 0.01));
    for role in ["AttackCleave", "AttackSlam"] {
        let clip = asset.clip_named(role).unwrap();
        let socket = asset.joint_named("BeamOrigin").unwrap();
        let mut point = |seconds| {
            asset.sample(clip, seconds, &mut pose);
            asset.joint_transform(&pose, socket).unwrap().transform_point3(glam::Vec3::ZERO)
        };
        let held = point(0.3);
        assert!(held.abs_diff_eq(point(0.4), 1e-5), "{role} does not hold anticipation");
        assert!(held.distance(point(0.6)) > 0.5, "{role} never releases");
        let follow_through = if role == "AttackCleave" { 0.3 } else { 0.1 };
        assert!(point(0.6).distance(point(0.7)) > follow_through,
            "{role} froze instead of carrying the strike through recovery");
        assert!(point(0.0).abs_diff_eq(point(1.0), 1e-5));
        if role == "AttackSlam" {
            assert!(held.y > 2.0, "slam sword must rise above the head: {held:?}");
            assert!(point(0.6).y < 0.8, "slam must drive toward the ground");
        }
    }
}

#[test]
fn cleave_has_independent_arm_arcs_and_a_pivot_at_the_sword_wrist() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let mut pose = asset.bind_pose();
    let clip = asset.clip_named("AttackCleave").unwrap();
    let joint = |name| asset.joint_named(name).unwrap();
    let position = |pose: &arpg_assets::CharacterPose, name| {
        asset.joint_transform(pose, joint(name)).unwrap().transform_point3(glam::Vec3::ZERO)
    };
    let relative = |pose: &arpg_assets::CharacterPose, parent, child| {
        asset.joint_transform(pose, joint(parent)).unwrap().inverse()
            * asset.joint_transform(pose, joint(child)).unwrap()
    };
    // The sword is a separate child hinge at the hand, not an attachment to
    // the torso or a rigid continuation of the forearm.
    let glb = gltf::Gltf::from_slice(BASIC_PLAYER).unwrap();
    let sword_hand = glb.nodes().find(|node| node.name() == Some("RightHand")).unwrap();
    assert!(sword_hand.children().any(|node| node.name() == Some("RightGrip")));
    for sample in 0..=120 {
        asset.sample(clip, sample as f32 / 120.0, &mut pose);
        let wrist = position(&pose, "RightGrip");
        let grip = position(&pose, "Weapon");
        assert!(grip.distance(wrist) < 0.001, "sword left its palm grasp at {sample}");

    }
    asset.sample(clip, 0.0, &mut pose);
    let ready = ["LeftArm", "RightArm"].map(|name| relative(&pose, "Chest", name));
    let ready_wrist = relative(&pose, "RightForearm", "Weapon").transform_vector3(glam::Vec3::Y).normalize();
    let ready_pelvis = position(&pose, "Pelvis");
    let ready_elbow = relative(&pose, "Chest", "RightForearm").transform_point3(glam::Vec3::ZERO);
    asset.sample(clip, 0.4, &mut pose);
    for (name, ready) in ["LeftArm", "RightArm"].into_iter().zip(ready) {
        let ready_axis = ready.transform_vector3(glam::Vec3::Y).normalize();
        let loaded_axis = relative(&pose, "Chest", name).transform_vector3(glam::Vec3::Y).normalize();
        assert!(ready_axis.angle_between(loaded_axis) > 0.2, "{name} follows the torso rigidly");
    }
    let loaded_elbow = relative(&pose, "Chest", "RightForearm").transform_point3(glam::Vec3::ZERO);
    assert!(ready_elbow.distance(loaded_elbow) > 0.25, "sword elbow never opens across the body");
    assert!(ready_pelvis.y - position(&pose, "Pelvis").y > 0.25, "loaded stance has no visible compression");
    assert!(position(&pose, "Head").z - position(&pose, "Pelvis").z > 0.15, "torso does not lead forward into the cut");
    let loaded_grip = relative(&pose, "RightHand", "Weapon");
    asset.sample(clip, 0.6, &mut pose);
    let released_wrist = relative(&pose, "RightForearm", "Weapon").transform_vector3(glam::Vec3::Y).normalize();
    assert!(ready_wrist.angle_between(released_wrist) > 0.7, "hand and blade stay locked to the forearm");
    assert!(loaded_grip.abs_diff_eq(relative(&pose, "RightHand", "Weapon"), 1e-5), "blade rotated inside the gripping hand");
    assert!(position(&pose, "RightHand").x > 0.7, "sword arm never extends outside the torso");
    let sword = asset.joint_transform(&pose, joint("Weapon")).unwrap();
    let free_hand = position(&pose, "LeftHand");
    asset.sample(clip, 0.7, &mut pose);
    assert!(!sword.abs_diff_eq(asset.joint_transform(&pose, joint("Weapon")).unwrap(), 0.1), "sword has no residual momentum");
    assert!(free_hand.distance(position(&pose, "LeftHand")) > 0.1, "free arm has no independent follow-through");
    assert!(loaded_grip.abs_diff_eq(relative(&pose, "RightHand", "Weapon"), 1e-5));
}

#[test]
fn swordsman_atlas_preserves_the_linear_steel_colour_through_srgb_decode() {
    let asset = import_character_glb(BASIC_PLAYER).unwrap();
    let body = asset.parts().iter().find(|part| part.node_name() == "Swordsman").unwrap();
    let image = asset.materials()[body.material_index()].image_index();
    let rgba = asset.images()[image].rgba8();
    let pixel = &rgba[(224 * 256 + 224) * 4..][..4];
    let linear = arpg_core::Srgb::from_rgb8([pixel[0], pixel[1], pixel[2]]).to_linear().to_array();
    let intended = glam::Vec3::new(0.13, 0.17, 0.19);
    assert!(glam::Vec3::from(linear).distance(intended) < 0.03, "steel colour was darkened by its PNG encoding: {linear:?}");
    assert_eq!(pixel[3], 255);
}
