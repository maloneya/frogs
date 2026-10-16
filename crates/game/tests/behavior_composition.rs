//! Predicted outcomes through the production schedule, using test-owned scenes.

use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{
    ActionDefinition, ActionKind, ActionOutcome, ActionTiming, ActorDefinition, BehaviorNode,
    Destination, Game, GameScene, MoveAction, Placed, Scene, SourceSpec, SwipeAction,
    TargetSelection,
};
use arpg_sim::{Accumulator, Condition, Dt, InteractionState, Template, Wall};
use glam::Vec2;
use std::num::NonZeroU8;

fn step(game: &mut Game) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}
fn scene(bodies: Vec<Placed>) -> GameScene {
    GameScene {
        engine: Scene {
            name: "behavior test".into(),
            bodies,
            ..Scene::default()
        },
        ..GameScene::default()
    }
}
fn wait_sequence() -> ActorDefinition {
    ActorDefinition {
        actions: vec![
            ActionDefinition::Wait { ticks: 2 },
            ActionDefinition::Wait { ticks: 3 },
        ],
        behavior: BehaviorNode::Sequence(vec![BehaviorNode::Action(0), BehaviorNode::Action(1)]),
        ..ActorDefinition::civilian()
    }
}

#[test]
fn civilian_and_hostile_use_the_same_move_action_without_sharing_hostility_or_health() {
    let follower = ActorDefinition {
        actions: vec![ActionDefinition::Move(MoveAction::default())],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![
        Placed {
            pos: (4.0, 0.0),
            what: ActorDefinition::hostile(),
        },
        Placed {
            pos: (-4.0, 0.0),
            what: follower,
        },
    ]))
    .unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    assert_eq!(game.enemy_count(), 1);
    assert_eq!(game.health(ids[0]), Some(3));
    assert_eq!(game.health(ids[1]), None);
    step(&mut game);
    assert!(game.body_pos(ids[0]).unwrap().x < 4.0);
    assert!(game.body_pos(ids[1]).unwrap().x > -4.0);
    assert_eq!(game.action_status(ids[0]).unwrap().kind, ActionKind::Move);
    assert_eq!(game.action_status(ids[1]).unwrap().kind, ActionKind::Move);
    assert_eq!(game.enemy_count(), 1);
}

#[test]
fn sequence_progress_belongs_to_each_instance_and_replays_exactly() {
    let setup = scene(vec![Placed {
        pos: (4.0, 0.0),
        what: wait_sequence(),
    }]);
    let mut game = Game::from_scene(&setup).unwrap();
    let mut replay = Game::from_scene(&setup).unwrap();
    let first = game.action_statuses().next().unwrap().body;
    for _ in 0..3 {
        step(&mut game);
        step(&mut replay);
    }
    assert_eq!(game.action_status(first).unwrap().action, Some(1));
    assert_eq!(game.action_status(first).unwrap().elapsed, 0);
    let second = game.place(Vec2::new(8.0, 0.0), wait_sequence()).unwrap();
    assert_eq!(
        replay.place(Vec2::new(8.0, 0.0), wait_sequence()),
        Some(second)
    );
    step(&mut game);
    step(&mut replay);
    assert_eq!(game.action_status(first).unwrap().action, Some(1));
    assert_eq!(game.action_status(first).unwrap().elapsed, 1);
    assert_eq!(game.action_status(second).unwrap().action, Some(0));
    assert_eq!(game.action_status(second).unwrap().elapsed, 0);
    for _ in 0..12 {
        step(&mut game);
        step(&mut replay);
        assert_eq!(game.hash(), replay.hash());
    }
    game.restart().unwrap();
    assert_eq!(game.action_statuses().count(), 1);
    assert_eq!(game.action_status(first).unwrap().kind, ActionKind::Idle);
}

#[test]
fn civilian_move_wait_interact_sequence_activates_through_the_shared_endpoint() {
    let routine = ActorDefinition {
        actions: vec![
            ActionDefinition::Move(MoveAction {
                destination: Destination::Point((6.0, 0.0)),
                stop_distance: 0.1,
                ..MoveAction::default()
            }),
            ActionDefinition::Wait { ticks: 3 },
            ActionDefinition::Interact {
                target: TargetSelection::NearestInteraction,
            },
        ],
        behavior: BehaviorNode::Sequence(vec![
            BehaviorNode::Action(0),
            BehaviorNode::Action(1),
            BehaviorNode::Action(2),
        ]),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![
        Placed {
            pos: (4.0, 0.0),
            what: routine,
        },
        Placed {
            pos: (7.0, 0.0),
            what: ActorDefinition::prop().interactive(),
        },
    ]))
    .unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    let mut waiting = None;
    let mut activated = None;
    for tick in 0..180 {
        step(&mut game);
        if game.action_status(ids[0]).unwrap().kind == ActionKind::Wait && waiting.is_none() {
            assert_eq!(
                game.action_status(ids[0]).unwrap().previous_outcome,
                ActionOutcome::Succeeded
            );
            waiting = Some(tick);
        }
        if game.interaction_state(ids[1]) == Some(InteractionState::Activated) {
            activated = Some(tick);
            break;
        }
    }
    assert_eq!(activated, waiting.map(|tick| tick + 3));
    assert!(
        activated.is_some(),
        "the NPC reached the point, waited, and activated"
    );
    assert_eq!(game.enemy_count(), 0);
}

#[test]
fn civilians_do_not_close_source_population_gates_and_births_act_immediately() {
    let mut setup = scene(vec![Placed {
        pos: (4.0, 0.0),
        what: ActorDefinition::civilian(),
    }]);
    setup.engine.sources.push(SourceSpec {
        pos: (8.0, 0.0),
        radius: 0.0,
        every: 1,
        enabled: true,
        when: Condition::FewerThan(1),
        what: ActorDefinition::hostile(),
    });
    let mut game = Game::from_scene(&setup).unwrap();
    step(&mut game);
    let born = game.spawn_outcomes()[0].entity.unwrap();
    assert_eq!(game.enemy_count(), 1);
    assert_eq!(game.action_status(born).unwrap().kind, ActionKind::Move);
    assert!(
        game.body_pos(born).unwrap().x < 8.0,
        "newborn acts on its birth tick"
    );
    for _ in 0..10 {
        step(&mut game);
    }
    assert_eq!(game.enemy_count(), 1);
}

#[test]
fn invalid_action_capabilities_and_references_refuse_before_mutation() {
    let mut game = Game::from_scene(&scene(vec![Placed {
        pos: (4.0, 0.0),
        what: ActorDefinition::attacker(),
    }]))
    .unwrap();
    step(&mut game);
    let id = game.action_statuses().next().unwrap().body;
    let before = game.hash();
    assert!(
        game.place(Vec2::splat(f32::MAX), ActorDefinition::hostile())
            .is_none()
    );
    assert_eq!(game.hash(), before);
    let invalid = [
        ActorDefinition {
            actions: vec![ActionDefinition::Move(MoveAction {
                destination: Destination::Point((f32::MAX, 0.0)),
                ..MoveAction::default()
            })],
            behavior: BehaviorNode::Action(0),
            ..ActorDefinition::civilian()
        },
        ActorDefinition {
            behavior: BehaviorNode::Action(99),
            ..ActorDefinition::attacker()
        },
        ActorDefinition {
            physical: Template::BLOCK,
            ..ActorDefinition::hostile()
        },
        ActorDefinition {
            behavior: BehaviorNode::Sequence(Vec::new()),
            ..ActorDefinition::civilian()
        },
        ActorDefinition {
            actions: vec![ActionDefinition::Move(MoveAction {
                acceleration: f32::NAN,
                ..MoveAction::default()
            })],
            behavior: BehaviorNode::Action(0),
            ..ActorDefinition::hostile()
        },
        ActorDefinition {
            actions: vec![ActionDefinition::Move(MoveAction {
                speed: 31.0,
                ..MoveAction::default()
            })],
            behavior: BehaviorNode::Action(0),
            ..ActorDefinition::hostile()
        },
    ];
    for definition in invalid {
        assert!(
            game.place(Vec2::new(9.0, 0.0), definition.clone())
                .is_none()
        );
        assert_eq!(game.hash(), before);
        assert!(game.assign_behavior(id, definition).is_err());
        assert_eq!(game.hash(), before);
    }
}

fn guard(startup: u32) -> ActorDefinition {
    ActorDefinition {
        actions: vec![ActionDefinition::Swipe(SwipeAction {
            target: TargetSelection::NearestHostile,
            timing: ActionTiming {
                startup,
                active: 1,
                recovery: 1,
            },
            ..SwipeAction::default()
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    }
}

#[test]
fn two_actions_hitting_the_same_lethal_target_in_one_tick_share_damage_admission() {
    let victim = ActorDefinition {
        physical: Template::BODY
            .character()
            .with_health(NonZeroU8::new(1).unwrap()),
        ..ActorDefinition::passive_hostile()
    };
    let mut game = Game::from_scene(&scene(vec![
        Placed {
            pos: (5.0, -0.4),
            what: guard(36),
        },
        Placed {
            pos: (5.0, 0.4),
            what: guard(24),
        },
        Placed {
            pos: (6.0, 0.0),
            what: victim,
        },
    ]))
    .unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    for tick in 0..=36 {
        step(&mut game);
        if tick < 36 {
            assert_eq!(game.health(ids[2]), Some(1));
        }
    }
    assert_eq!(game.health(ids[2]), Some(0));
    assert_eq!(game.enemy_count(), 0);
    assert_eq!(game.action_statuses().count(), 2);
    for _ in 0..120 { step(&mut game); }
    assert_eq!(game.health(ids[2]), None);
}

#[test]
fn target_loss_preserves_physical_commitment_but_definition_replacement_cancels() {
    let setup = scene(vec![
        Placed {
            pos: (5.0, 0.0),
            what: guard(36),
        },
        Placed {
            pos: (6.0, 0.0),
            what: ActorDefinition::passive_hostile(),
        },
    ]);
    let mut game = Game::from_scene(&setup).unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    step(&mut game);
    assert_eq!(game.action_status(ids[0]).unwrap().target, Some(ids[1]));
    assert!(game.despawn_body(ids[1]));
    let replacement = game
        .place(Vec2::new(6.0, 0.0), ActorDefinition::prop())
        .unwrap();
    assert_ne!(replacement, ids[1]);
    step(&mut game);
    let status = game.action_status(ids[0]).unwrap();
    assert_eq!(status.kind, ActionKind::Swipe);
    assert_eq!(status.outcome, ActionOutcome::Running);
    assert_eq!(status.target, Some(ids[1]));
    for _ in 2..=36 { step(&mut game); }
    assert!(game.trace().since(0).any(|(_, event)| matches!(event,
        arpg_sim::Event::Impulsed { id, source: Some(source), .. } if id == replacement && source == ids[0])));
    assert_eq!(game.health(replacement), None);
    game.restart().unwrap();
    step(&mut game);
    game.assign_behavior(ids[0], ActorDefinition::civilian())
        .unwrap();
    assert_eq!(
        game.action_status(ids[0]).unwrap().outcome,
        ActionOutcome::Cancelled
    );
    for _ in 0..84 {
        step(&mut game);
    }
    assert_eq!(game.health(ids[1]), Some(3));
}

#[test]
fn a_refused_interaction_race_restarts_the_failed_nested_sequence() {
    let definition = ActorDefinition {
        actions: vec![
            ActionDefinition::Wait { ticks: 2 },
            ActionDefinition::Interact {
                target: TargetSelection::NearestInteraction,
            },
        ],
        behavior: BehaviorNode::Sequence(vec![BehaviorNode::Sequence(vec![
            BehaviorNode::Action(0),
            BehaviorNode::Action(1),
        ])]),
        ..ActorDefinition::civilian()
    };
    let setup = scene(vec![
        Placed {
            pos: (5.0, -0.4),
            what: definition.clone(),
        },
        Placed {
            pos: (5.0, 0.4),
            what: definition,
        },
        Placed {
            pos: (6.0, 0.0),
            what: ActorDefinition::prop().interactive(),
        },
    ]);
    let mut game = Game::from_scene(&setup).unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    for _ in 0..3 {
        step(&mut game);
    }
    assert_eq!(
        game.interaction_state(ids[2]),
        Some(InteractionState::Activated)
    );
    assert_eq!(
        game.action_status(ids[0]).unwrap().outcome,
        ActionOutcome::Succeeded
    );
    assert_eq!(
        game.action_status(ids[1]).unwrap().outcome,
        ActionOutcome::Failed
    );
    step(&mut game);
    let status = game.action_status(ids[1]).unwrap();
    assert_eq!(status.kind, ActionKind::Wait);
    assert_eq!(
        status.elapsed, 0,
        "the failed nested sequence restarts its wait"
    );
    assert_eq!(status.previous_outcome, ActionOutcome::Failed);
}

#[test]
fn movement_releases_a_target_that_loses_its_required_role() {
    let follower = ActorDefinition {
        actions: vec![ActionDefinition::Move(MoveAction {
            destination: Destination::Target(TargetSelection::NearestHostile),
            ..MoveAction::default()
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![
        Placed {
            pos: (4.0, 0.0),
            what: follower,
        },
        Placed {
            pos: (8.0, 0.0),
            what: ActorDefinition::passive_hostile(),
        },
    ]))
    .unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    step(&mut game);
    assert_eq!(game.action_status(ids[0]).unwrap().target, Some(ids[1]));
    let peaceful = ActorDefinition {
        role: arpg_game::ActorRole::Civilian,
        ..ActorDefinition::passive_hostile()
    };
    game.assign_behavior(ids[1], peaceful).unwrap();
    step(&mut game);
    assert_eq!(game.action_status(ids[0]).unwrap().kind, ActionKind::Idle);
    assert_eq!(
        game.action_status(ids[0]).unwrap().outcome,
        ActionOutcome::Cancelled
    );
}

#[test]
fn movement_releases_an_endpoint_activated_by_another_actor() {
    let follower = ActorDefinition {
        actions: vec![ActionDefinition::Move(MoveAction {
            destination: Destination::Target(TargetSelection::NearestInteraction),
            ..MoveAction::default()
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    };
    let interactor = ActorDefinition {
        actions: vec![ActionDefinition::Interact {
            target: TargetSelection::NearestInteraction,
        }],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![
        Placed {
            pos: (3.0, 0.0),
            what: follower,
        },
        Placed {
            pos: (6.5, 0.0),
            what: interactor,
        },
        Placed {
            pos: (6.0, 0.0),
            what: ActorDefinition::prop().interactive(),
        },
    ]))
    .unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    step(&mut game);
    assert_eq!(game.action_status(ids[0]).unwrap().kind, ActionKind::Move);
    assert_eq!(
        game.interaction_state(ids[2]),
        Some(InteractionState::Activated)
    );
    step(&mut game);
    assert_eq!(game.action_status(ids[0]).unwrap().kind, ActionKind::Idle);
    assert_eq!(
        game.action_status(ids[0]).unwrap().outcome,
        ActionOutcome::Cancelled
    );
}

#[test]
fn higher_priority_attack_cancels_movement_when_its_opportunity_opens() {
    let mut game = Game::from_scene(&scene(vec![Placed {
        pos: (8.0, 0.0),
        what: ActorDefinition::attacker(),
    }]))
    .unwrap();
    let id = game.action_statuses().next().unwrap().body;
    step(&mut game);
    assert_eq!(game.action_status(id).unwrap().kind, ActionKind::Move);
    let mut leaped = false;
    for _ in 0..90 {
        step(&mut game);
        let status = game.action_status(id).unwrap();
        if status.kind == ActionKind::Leap {
            assert_eq!(status.previous_outcome, ActionOutcome::Cancelled);
            assert_eq!(status.elapsed, 0);
            leaped = true;
            break;
        }
    }
    assert!(leaped);
}

#[test]
fn maximum_speed_diagonal_actions_remain_valid_when_velocity_is_rounded() {
    let actor = ActorDefinition {
        actions: vec![ActionDefinition::Move(MoveAction {
            destination: Destination::Point((100.0, 100.0)),
            speed: 30.0,
            acceleration: 300.0,
            stop_distance: 0.1,
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![Placed {
        pos: (8.0, 8.0),
        what: actor,
    }]))
    .unwrap();
    for _ in 0..5 {
        step(&mut game);
    }
    let id = game.action_statuses().next().unwrap().body;
    let velocity = game.motion(id).unwrap().velocity();
    assert!(velocity.is_finite());
    assert!(velocity.length_squared() <= 900.0);
}

#[test]
fn arrival_keeps_the_motor_until_bounded_braking_has_stopped_the_body() {
    let actor = ActorDefinition {
        actions: vec![ActionDefinition::Move(MoveAction {
            destination: Destination::Point((4.0, 0.0)),
            speed: 1.0,
            acceleration: 6.0,
            stop_distance: 1.0,
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    };
    for sequence in [false, true] {
        let mut actor = actor.clone();
        if sequence {
            actor.actions.push(ActionDefinition::Wait { ticks: 3 });
            actor.behavior = BehaviorNode::Sequence(vec![BehaviorNode::Action(0), BehaviorNode::Action(1)]);
        }
        let mut game = Game::from_scene(&scene(vec![Placed { pos: (4.0, 0.0), what: actor }])).unwrap();
        let id = game.action_statuses().next().unwrap().body;
        assert!(game.apply_impulse(id, arpg_sim::Impulse::try_from((0.2, 0.0)).unwrap()));
        // Already inside the arrival radius, but 0.2m/s cannot be stopped by the
        // first tick's 0.1m/s brake. Drag leaves 0.18m/s, then the motor leaves 0.08.
        // The whole change ramps in over four substeps: the tick covers
        // (0.2 - 5/8 0.12) dt.
        step(&mut game);
        assert_eq!(game.action_status(id).unwrap().kind, ActionKind::Move);
        assert!((game.motion(id).unwrap().velocity().x - 0.08).abs() < 1e-6);
        assert!((game.body_pos(id).unwrap().x - (4.0 + 0.125 / 60.0)).abs() < 1e-6);
        step(&mut game);
        assert_eq!(game.action_status(id).unwrap().kind, ActionKind::Move);
        assert_eq!(game.motion(id).unwrap().velocity(), Vec2::ZERO);
        let stopped = game.body_pos(id);
        for elapsed in 0..3 {
            step(&mut game);
            let status = game.action_status(id).unwrap();
            if sequence {
                assert_eq!(status.kind, ActionKind::Wait);
                assert_eq!(status.elapsed, elapsed);
                assert_eq!(status.previous_outcome, ActionOutcome::Succeeded);
            } else {
                assert_eq!(status.kind, ActionKind::Idle);
                assert_eq!(status.outcome, ActionOutcome::Succeeded);
            }
            assert_eq!(game.body_pos(id), stopped);
        }
    }
}

#[test]
fn nested_sequences_propagate_completion_without_skipping_or_restarting_children() {
    let actor = ActorDefinition {
        actions: vec![
            ActionDefinition::Wait { ticks: 1 },
            ActionDefinition::Wait { ticks: 2 },
            ActionDefinition::Wait { ticks: 3 },
        ],
        behavior: BehaviorNode::Sequence(vec![
            BehaviorNode::Priority(vec![
                BehaviorNode::Idle,
                BehaviorNode::Sequence(vec![BehaviorNode::Action(0), BehaviorNode::Action(1)]),
            ]),
            BehaviorNode::Action(2),
        ]),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![Placed {
        pos: (5.0, 0.0),
        what: actor,
    }]))
    .unwrap();
    let id = game.action_statuses().next().unwrap().body;
    for (action, elapsed) in [(0, 0), (1, 0), (1, 1), (2, 0), (2, 1), (2, 2), (0, 0)] {
        step(&mut game);
        let status = game.action_status(id).unwrap();
        assert_eq!((status.action, status.elapsed), (Some(action), elapsed));
        if action != 0 {
            assert_eq!(status.previous_outcome, ActionOutcome::Succeeded);
        }
    }
}

#[test]
fn a_completed_interaction_advances_its_sequence_after_the_endpoint_changes_state() {
    let actor = ActorDefinition {
        actions: vec![
            ActionDefinition::Interact {
                target: TargetSelection::NearestInteraction,
            },
            ActionDefinition::Wait { ticks: 2 },
        ],
        behavior: BehaviorNode::Sequence(vec![BehaviorNode::Action(0), BehaviorNode::Action(1)]),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![
        Placed {
            pos: (5.0, 0.0),
            what: actor,
        },
        Placed {
            pos: (6.0, 0.0),
            what: ActorDefinition::prop().interactive(),
        },
    ]))
    .unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    step(&mut game);
    assert_eq!(
        game.interaction_state(ids[1]),
        Some(InteractionState::Activated)
    );
    assert_eq!(
        game.action_status(ids[0]).unwrap().outcome,
        ActionOutcome::Succeeded
    );
    assert!(
        game.despawn_body(ids[1]),
        "a completed endpoint can disappear before the next decision"
    );
    step(&mut game);
    let status = game.action_status(ids[0]).unwrap();
    assert_eq!((status.action, status.elapsed), (Some(1), 0));
    assert_eq!(status.previous_outcome, ActionOutcome::Succeeded);
}

#[test]
fn interrupting_a_nested_sequence_discards_its_abandoned_progress() {
    let actor = ActorDefinition {
        actions: vec![
            ActionDefinition::Swipe(SwipeAction {
                target: TargetSelection::NearestHostile,
                timing: ActionTiming {
                    startup: 1,
                    active: 1,
                    recovery: 2,
                },
                ..SwipeAction::default()
            }),
            ActionDefinition::Wait { ticks: 2 },
            ActionDefinition::Move(MoveAction {
                destination: Destination::Point((10.0, 0.0)),
                ..MoveAction::default()
            }),
            ActionDefinition::Wait { ticks: 3 },
        ],
        behavior: BehaviorNode::Priority(vec![
            BehaviorNode::Action(0),
            BehaviorNode::Sequence(vec![
                BehaviorNode::Action(1),
                BehaviorNode::Action(2),
                BehaviorNode::Action(3),
            ]),
        ]),
        ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&scene(vec![Placed {
        pos: (5.0, 0.0),
        what: actor,
    }]))
    .unwrap();
    let id = game.action_statuses().next().unwrap().body;
    for _ in 0..3 {
        step(&mut game);
    }
    assert_eq!(game.action_status(id).unwrap().action, Some(2));
    let victim = ActorDefinition {
        physical: Template::BODY
            .character()
            .with_health(NonZeroU8::new(1).unwrap()),
        ..ActorDefinition::passive_hostile()
    };
    let origin = game.body_pos(id).unwrap();
    let target = game
        .place(Vec2::new(origin.x + 1.0, origin.z), victim)
        .unwrap();
    step(&mut game);
    let status = game.action_status(id).unwrap();
    assert_eq!(status.kind, ActionKind::Swipe);
    assert_eq!(status.previous_outcome, ActionOutcome::Cancelled);
    step(&mut game);
    assert_eq!(game.health(target), Some(0));
    step(&mut game);
    assert_eq!(game.health(target), Some(0));
    assert_eq!(game.action_status(id).unwrap().phase, arpg_sim::AttackPhase::Recovery,
        "defeating the target does not skip physical action recovery");
    step(&mut game);
    step(&mut game);
    let status = game.action_status(id).unwrap();
    assert_eq!(
        (status.action, status.elapsed),
        (Some(1), 0),
        "after recovery the interrupted sequence restarts its first wait"
    );
}

#[test]
fn priority_skips_an_interaction_blocked_by_known_cover() {
    let actor = ActorDefinition {
        actions: vec![
            ActionDefinition::Interact {
                target: TargetSelection::NearestInteraction,
            },
            ActionDefinition::Wait { ticks: 2 },
        ],
        behavior: BehaviorNode::Priority(vec![BehaviorNode::Action(0), BehaviorNode::Action(1)]),
        ..ActorDefinition::civilian()
    };
    let mut setup = scene(vec![
        Placed {
            pos: (5.0, 0.0),
            what: actor,
        },
        Placed {
            pos: (6.0, 0.0),
            what: ActorDefinition::prop().interactive(),
        },
    ]);
    setup
        .engine
        .walls
        .push(Wall::new(Vec2::new(5.5, 0.0), Vec2::new(0.1, 3.0), 0.0).unwrap());
    let mut game = Game::from_scene(&setup).unwrap();
    let ids: Vec<_> = game.action_statuses().map(|status| status.body).collect();
    step(&mut game);
    assert_eq!(game.action_status(ids[0]).unwrap().kind, ActionKind::Wait);
    assert_eq!(game.action_status(ids[0]).unwrap().elapsed, 0);
    assert_eq!(
        game.interaction_state(ids[1]),
        Some(InteractionState::Ready)
    );
}
