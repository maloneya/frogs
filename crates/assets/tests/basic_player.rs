//! Contract test for the Blender-authored basic player asset.

use arpg_assets::import_character_glb;

const BASIC_PLAYER: &[u8] =
    include_bytes!("../../../assets/characters/basic-player/basic-player.glb");

#[test]
fn basic_player_satisfies_the_player_presentation_contract() {
    let character = import_character_glb(BASIC_PLAYER).unwrap();

    assert!(character.vertex_count() > 300);
    assert!(character.index_count() > 500);
    assert_eq!(character.base_color_texture().width(), 64);
    assert_eq!(character.base_color_texture().height(), 64);
    assert_eq!(character.joint_count(), 8);
    assert_eq!(character.clip_count(), 4);
    assert!(character.joint_named("Weapon").is_some());

    for name in [
        "Idle",
        "Run",
        "AttackCleave",
        "AttackSlam",
    ] {
        assert!(character.clip_named(name).is_some(), "missing {name}");
    }
}
