//! Contract test for the Blender-authored basic horde asset.

use arpg_assets::import_character_glb;

const BASIC_ENEMY: &[u8] =
    include_bytes!("../../../assets/characters/basic-enemy/basic-enemy.glb");

#[test]
fn basic_enemy_satisfies_the_horde_presentation_contract() {
    let character = import_character_glb(BASIC_ENEMY).unwrap();

    assert!(character.vertex_count() > 250);
    assert!(character.index_count() > 400);
    assert_eq!(character.images()[0].width(), 64);
    assert_eq!(character.images()[0].height(), 64);
    assert_eq!(character.joint_count(), 7);
    assert!(character.clip_named("Idle").is_some());
    assert!(character.clip_named("Run").is_some());
}
