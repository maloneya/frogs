//! Every birth path installs the complete definition before first-tick decisions.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{
    ActionDefinition, ActionKind, ActorDefinition, BehaviorNode, BodyGrid, Game, GameScene, Placed,
    Scene, SourceSpec, SwipeAction, TargetSelection,
};
use arpg_sim::{Accumulator, Condition, Dt, Event};
use glam::Vec2;
use std::num::NonZeroU8;
fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}
#[test]
fn direct_grid_source_and_queued_births_act_on_their_first_tick() {
    let definition = ActorDefinition::hostile();
    let scene: GameScene = Scene {
        bodies: vec![Placed {
            pos: (20.0, 0.0),
            what: definition.clone(),
        }],
        grids: vec![BodyGrid {
            origin: (-20.0, 10.0),
            columns: 2,
            rows: 1,
            spacing: 1.0,
            what: definition.clone(),
        }],
        sources: vec![SourceSpec {
            enabled: true,
            pos: (0.0, -20.0),
            radius: 0.0,
            every: 60,
            when: Condition::Always,
            what: definition.clone(),
        }],
        ..Default::default()
    }
    .into();
    let mut game = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let direct = game
        .place(Vec2::new(-20.0, 0.0), definition.clone())
        .unwrap();
    let ticket = game
        .request_spawn(Vec2::new(0.0, 20.0), definition)
        .unwrap();
    step(&mut game);
    let queued = game
        .spawn_outcomes()
        .iter()
        .find(|outcome| outcome.ticket == ticket)
        .unwrap()
        .entity
        .unwrap();
    assert_eq!(game.enemy_count(), 6);
    assert_eq!(
        game.scene_bodies(owner).unwrap().len(),
        4,
        "source descendants retain ownership"
    );
    for id in game
        .scene_bodies(owner)
        .unwrap()
        .iter()
        .copied()
        .chain([direct, queued])
    {
        assert_eq!(game.action_status(id).unwrap().kind, ActionKind::Move);
        assert!(
            (game.motion(id).unwrap().velocity().length() - 0.5).abs() < 1e-5,
            "first tick includes the motor"
        );
    }
    assert!(game.evict_scene(owner));
    assert_eq!(game.enemy_count(), 2);
    assert!(game.action_status(direct).is_some());
    assert!(game.action_status(queued).is_some());
    game.restart().unwrap();
    assert_eq!(game.hash(), Game::from_scene(&scene).unwrap().hash());
}
fn defender(startup: u32) -> ActorDefinition {
    let mut strike = SwipeAction {
        target: TargetSelection::NearestHostile,
        ..Default::default()
    };
    strike.timing.startup = startup;
    strike.damage = NonZeroU8::new(3).unwrap();
    ActorDefinition {
        actions: vec![ActionDefinition::Swipe(strike)],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    }
}
#[test]
fn simultaneous_lethal_effects_produce_one_defeat_and_no_second_hit() {
    let mut game = Game::empty();
    let first = game.place(Vec2::new(-0.4, 10.0), defender(20)).unwrap();
    let second = game.place(Vec2::new(0.4, 10.0), defender(8)).unwrap();
    let target = game
        .place(Vec2::new(0.0, 11.0), ActorDefinition::passive_hostile())
        .unwrap();
    for _ in 0..20 {
        step(&mut game);
        assert_eq!(game.health(target), Some(3));
    }
    step(&mut game);
    assert_eq!(game.defeats().len(), 1);
    assert_eq!(game.defeats()[0].body, target);
    assert!(!game.is_alive(target));
    assert_eq!(game.enemy_count(), 0);
    // The first commitment starts at 0 with 20 startup ticks; the second
    // starts at 12 with eight. Both retain nine active and 39 recovery ticks
    // after losing their target, completing together at tick 68.
    for tick in 21..68 {
        step(&mut game);
        assert!(game.defeats().is_empty());
        for actor in [first, second] {
            assert_eq!(game.action_status(actor).unwrap().kind, ActionKind::Swipe,
                "committed swing ended before tick 68, at {tick}");
        }
    }
    step(&mut game);
    assert!(game.defeats().is_empty());
    assert_eq!(game.action_status(first).unwrap().kind, ActionKind::Idle);
    assert_eq!(game.action_status(second).unwrap().kind, ActionKind::Idle);
    let hits: Vec<_> = game
        .trace()
        .iter()
        .filter_map(|(tick, event)| match event {
            Event::Damaged {
                actor,
                target: hit,
                remaining,
            } if hit == target => Some((tick, actor, remaining)),
            _ => None,
        })
        .collect();
    assert_eq!(hits, [(20, first, 0)]);
}

fn report(game: &Game) -> String {
    let mut report = arpg_core::Report::default();
    game.report(&mut report);
    report.finish()
}
fn waiter(ticks: u32) -> ActorDefinition {
    ActorDefinition {
        actions: vec![ActionDefinition::Wait { ticks }],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    }
}
#[test]
fn reports_distinguish_future_definitions_and_refusal_retains_accepted_births() {
    let mut one = Game::empty();
    let mut two = Game::empty();
    let ticket = one.request_spawn(Vec2::new(10.0, 0.0), waiter(1)).unwrap();
    two.request_spawn(Vec2::new(10.0, 0.0), waiter(2)).unwrap();
    assert_ne!(one.hash(), two.hash());
    assert_ne!(report(&one), report(&two));
    let accepted = (one.hash(), report(&one));
    let mut invalid = waiter(1);
    invalid.behavior = BehaviorNode::Action(1);
    assert!(
        one.request_spawn(Vec2::new(11.0, 0.0), invalid.clone())
            .is_none()
    );
    assert!(one.place(Vec2::new(11.0, 0.0), invalid).is_none());
    assert_eq!((one.hash(), report(&one)), accepted);
    step(&mut one);
    assert_eq!(one.spawn_outcomes()[0].ticket, ticket);
    let body = one.spawn_outcomes()[0].entity.unwrap();
    assert_eq!(one.action_status(body).unwrap().kind, ActionKind::Wait);
    let source = |ticks| {
        arpg_game::Source::try_from(SourceSpec {
            enabled: false,
            pos: (0.0, 20.0),
            radius: 0.0,
            every: 1,
            when: Condition::Always,
            what: waiter(ticks),
        })
        .unwrap()
    };
    let mut one = Game::empty();
    let mut two = Game::empty();
    let _ = one.add_source(source(1));
    let _ = two.add_source(source(2));
    assert_ne!(one.hash(), two.hash());
    assert_ne!(report(&one), report(&two));
}
