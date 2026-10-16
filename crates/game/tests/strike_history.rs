//! Broad physical strikes retain one hit per identity and release history with
//! their combat commitment, independent of the number of actors in the world.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{
    ActionDefinition, ActionTiming, ActorDefinition, BehaviorNode, Game, GameScene, Placed,
    SwipeAction,
};
use arpg_sim::{Accumulator, Dt, EntityId, Event};
use glam::Vec2;

fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}
fn attacker() -> ActorDefinition {
    ActorDefinition {
        actions: vec![ActionDefinition::Swipe(SwipeAction {
            reach: 64.0,
            offset: 45.0,
            radius: 64.0,
            impulse: 0.0,
            timing: ActionTiming {
                startup: 1,
                active: 4,
                recovery: 1,
            },
            ..Default::default()
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::attacker()
    }
}
fn hits(game: &Game, actor: EntityId) -> Vec<EntityId> {
    game.trace()
        .since(0)
        .filter_map(|(_, event)| match event {
            Event::Impulsed {
                id,
                source: Some(source),
                ..
            } if source == actor => Some(id),
            _ => None,
        })
        .collect()
}
#[test]
fn broad_swipes_hit_thousands_once_then_start_with_empty_history() {
    let mut setup = GameScene::default();
    setup.engine.bodies.push(Placed {
        pos: (0.0, 45.0),
        what: attacker(),
    });
    for z in 0..100 {
        for x in 0..100 {
            let pos = ((x as f32 - 49.5) * 0.6, (z as f32 - 49.5) * 0.6);
            // Keep the player outside every fixed footprint; all 9,996 props
            // lie inside the 64m swipe and outside each other's collision discs.
            if pos.0.abs() < 0.6 && pos.1.abs() < 0.6 {
                continue;
            }
            setup.engine.bodies.push(Placed {
                pos,
                what: ActorDefinition::prop(),
            });
        }
    }
    let mut game = Game::from_scene(&setup).unwrap();
    let mut replay = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let actor = game.scene_bodies(owner).unwrap()[0];
    let expected: Vec<_> = std::iter::once(game.player_id())
        .chain(game.scene_bodies(owner).unwrap()[1..].iter().copied())
        .collect();
    assert_eq!(expected.len(), 9_997);
    for tick in 0..17 {
        game.clear_trace();
        step(&mut game);
        step(&mut replay);
        assert_eq!(game.hash(), replay.hash());
        // The action starts at 0, impacts at 1, completes at 6. Coordination
        // admits the next action at 12, which must strike every identity again.
        let actual = hits(&game, actor);
        if matches!(tick, 1 | 13) {
            assert_eq!(actual, expected, "tick {tick}");
        } else {
            assert!(actual.is_empty(), "tick {tick}");
        }
    }
}

#[test]
fn cancellation_removal_and_scene_eviction_release_combat_histories() {
    let mut definition = attacker();
    definition.role = arpg_game::ActorRole::Civilian;
    let ActionDefinition::Swipe(swipe) = &mut definition.actions[0] else {
        unreachable!()
    };
    swipe.target = arpg_game::TargetSelection::NearestHostile;
    let mut setup = GameScene::default();
    setup.engine.bodies.push(Placed {
        pos: (0.0, 45.0),
        what: definition.clone(),
    });
    setup.engine.bodies.push(Placed {
        pos: (2.0, 0.0),
        what: ActorDefinition {
            physical: arpg_sim::Template::BLOCK.with_health(std::num::NonZeroU8::new(255).unwrap()),
            ..ActorDefinition::passive_hostile()
        },
    });
    let mut game = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let mut actor = game.scene_bodies(owner).unwrap()[0];
    let target = game.scene_bodies(owner).unwrap()[1];
    // More sequential commitments than the shared pool has slots. Every edit
    // happens during the active window, exercising cancellation and slot reuse.
    for attempt in 0..6 {
        game.clear_trace();
        let mut struck = false;
        for _ in 0..16 {
            step(&mut game);
            if hits(&game, actor).contains(&target) {
                struck = true;
                break;
            }
        }
        assert!(struck, "commitment {attempt}");
        if attempt % 2 == 0 {
            let idle = ActorDefinition {
                actions: vec![],
                behavior: BehaviorNode::Idle,
                ..definition.clone()
            };
            game.assign_behavior(actor, idle).unwrap();
            game.assign_behavior(actor, definition.clone()).unwrap();
        } else {
            assert!(game.despawn_body(actor));
            actor = game
                .place(Vec2::new(0.0, 45.0), definition.clone())
                .unwrap();
        }
    }
    assert!(game.despawn_body(actor));
    assert!(game.evict_scene(owner));
    // Repeated load/evict while active must not leave reservation histories.
    for _ in 0..6 {
        let owner = game.load_scene(&setup).unwrap();
        actor = game.scene_bodies(owner).unwrap()[0];
        let target = game.scene_bodies(owner).unwrap()[1];
        game.clear_trace();
        step(&mut game);
        step(&mut game);
        assert!(hits(&game, actor).contains(&target));
        assert!(game.evict_scene(owner));
    }
    game.restart().unwrap();
    assert_eq!(game.hash(), Game::from_scene(&setup).unwrap().hash());
    assert_eq!(
        game.motion(game.player_id()).unwrap().velocity(),
        Vec2::ZERO
    );
}

#[test]
fn defeat_releases_an_active_history_before_another_actor_commits() {
    let mut definition = attacker();
    let ActionDefinition::Swipe(swipe) = &mut definition.actions[0] else {
        unreachable!()
    };
    swipe.target = arpg_game::TargetSelection::NearestHostile;
    swipe.offset = 0.0;
    swipe.timing.active = 100;
    let mut game = Game::empty();
    let target = game
        .place(
            Vec2::new(2.0, 1.4),
            ActorDefinition {
                physical: arpg_sim::Template::BLOCK
                    .with_health(std::num::NonZeroU8::new(255).unwrap()),
                ..ActorDefinition::passive_hostile()
            },
        )
        .unwrap();
    game.set_attack_profile(arpg_sim::AttackProfile::Disintegrate);
    for attempt in 0..6 {
        let actor = game.place(Vec2::new(0.0, 1.4), definition.clone()).unwrap();
        game.clear_trace();
        for _ in 0..40 {
            game.step(
                Accumulator::default().pending(Dt::SECS).next().unwrap(),
                Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true),
            );
            if game.health(actor) == Some(0) {
                break;
            }
        }
        assert!(
            game.health(actor) == Some(0),
            "beam must defeat active producer {attempt}"
        );
        assert!(game.action_status(actor).is_none());
        assert_eq!(
            hits(&game, actor)
                .iter()
                .filter(|&&id| id == target)
                .count(),
            1
        );
        // Release the beam so the defeated producer's physics can settle.
        for _ in 0..120 { step(&mut game); }
        assert!(!game.is_alive(actor));
    }
}
