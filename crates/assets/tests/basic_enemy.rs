//! Contract test for the Blender-authored basic horde asset.

use arpg_assets::import_character_glb;

const BASIC_ENEMY: &[u8] =
    include_bytes!("../../../assets/characters/basic-enemy/basic-enemy.glb");
const SHADOW: &[u8] = include_bytes!("../../../assets/characters/basic-enemy/basic-enemy.shadow.glb");

#[test]
fn coarse_shadow_shares_the_visible_palette_without_changing_visible_geometry() {
    let mut visible = import_character_glb(BASIC_ENEMY).unwrap();
    let original = bytemuck::cast_slice::<_, u8>(visible.vertices()).to_vec();
    let indices = visible.indices().to_vec();
    let proxy = import_character_glb(SHADOW).unwrap();
    for role in ["Idle", "Run"] {
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
fn basic_enemy_satisfies_the_horde_presentation_contract() {
    let character = import_character_glb(BASIC_ENEMY).unwrap();

    assert!(character.vertex_count() > 250);
    assert!(character.index_count() > 400);
    assert_eq!(character.images()[0].width(), 64);
    assert_eq!(character.images()[0].height(), 64);
    assert_eq!(character.joint_count(), 19);
    assert!(character.clip_named("Idle").is_some());
    assert!(character.clip_named("Run").is_some());
    assert!(character.clip_named("Hit").is_some());
    assert!(character.clip_named("Death").is_some());
}
