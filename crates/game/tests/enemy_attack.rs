//! The first enemy-damage relationship through Game's public lifecycle.

use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{EnemyAttackSpec, Game, GameScene, SceneError, PLAYER_MAX_HEALTH};
use arpg_sim::{Accumulator, Dt, Placed, Scene, Template};

fn attacker_scene(name: &str) -> GameScene {
    GameScene {
        engine: Scene {
            name: name.into(),
            bodies: vec![Placed { pos: (0.0, 0.55), what: Template::BODY.seeking() }],
            ..Scene::default()
        },
        enemy_attack: Some(EnemyAttackSpec { body: 0 }),
        ..GameScene::default()
    }
}

fn step(game: &mut Game) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}

#[test]
fn rejected_attack_relationships_preserve_the_live_game() {
    let mut game = Game::from_scene(&attacker_scene("first")).unwrap();
    let before = game.hash();

    let second = attacker_scene("second");
    assert_eq!(
        game.load_scene(&second),
        Err(SceneError::EnemyAttack("only one enemy attacker may be live"))
    );
    assert_eq!(game.hash(), before);

    let invalid = GameScene {
        engine: Scene {
            name: "not a seeker".into(),
            bodies: vec![Placed { pos: (0.0, 3.0), what: Template::BODY }],
            ..Scene::default()
        },
        enemy_attack: Some(EnemyAttackSpec { body: 0 }),
        ..GameScene::default()
    };
    let mut empty = Game::empty();
    assert_eq!(
        empty.load_scene(&invalid),
        Err(SceneError::EnemyAttack(
            "enemy attacker must bind an explicit seeking body"
        ))
    );
    assert_eq!(empty.enemy_count(), 0);
}

#[test]
fn stale_attackers_retire_and_restart_restores_health() {
    let scene = attacker_scene("first");
    let mut game = Game::from_scene(&scene).unwrap();
    step(&mut game);
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH - 1);

    game.restart().unwrap();
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);
    let owner = game.scene_instances().next().unwrap().0;
    let body = game.scene_bodies(owner).unwrap()[0];
    assert!(game.despawn_body(body));
    assert_eq!(game.player_health(), PLAYER_MAX_HEALTH);

    assert!(game.load_scene(&attacker_scene("replacement")).is_ok());
}
