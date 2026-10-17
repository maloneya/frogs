//! Contact imparts momentum independently from combat eligibility and health.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{DoorPanel, ActionDefinition, ActionTiming, ActorDefinition, ActorRole, BehaviorNode, Game, GameScene, Placed, Scene, SwipeAction};
use arpg_sim::{Accumulator, Dt, EntityId, Event, Template};
use arpg_game::{AttackProfile};
use glam::Vec2;
use std::num::NonZeroU8;

fn step(game: &mut Game, press: bool, held: bool) {
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, press).with_attack_held(held));
}

fn impulses(game: &Game, actor: EntityId, target: EntityId) -> usize {
    game.trace().since(0).filter(|(_, event)| matches!(*event,
        Event::Impulsed { id, source: Some(source), .. } if source == actor && id == target)).count()
}

fn passive(role: ActorRole, health: bool) -> ActorDefinition {
    ActorDefinition {
        role,
        physical: if health { Template::BODY.with_health(NonZeroU8::new(10).unwrap()) } else { Template::BODY },
        actions: vec![],
        behavior: BehaviorNode::Idle,
    }
}

#[test]
fn player_melee_hits_neutral_and_healthless_bodies_once_without_changing_damage_policy() {
    for (role, health, expected_health) in [
        (ActorRole::Civilian, false, None),
        (ActorRole::Civilian, true, Some(10)),
        (ActorRole::Hostile, true, Some(8)),
    ] {
        let mut game = Game::empty();
        let target = game.place(Vec2::new(0.0, 1.5), passive(role, health)).unwrap();
        game.set_attack_profile(AttackProfile::Slam);
        let timing = game.attack_status().resolved.unwrap();
        step(&mut game, true, false);
        for _ in 1..timing.startup() { step(&mut game, false, false); }
        assert_eq!(impulses(&game, game.player_id(), target), 0, "startup cannot impact");
        step(&mut game, false, false);
        assert_eq!(impulses(&game, game.player_id(), target), 1);
        assert!(game.motion(target).unwrap().velocity().y > 0.0);
        assert_eq!(game.health(target), expected_health);
        for _ in 1..timing.active() { step(&mut game, false, false); }
        assert_eq!(impulses(&game, game.player_id(), target), 1, "one physical impact per swing");
    }
}

#[test]
fn beam_applies_physical_impulse_every_touching_tick_without_damage_membership() {
    for (role, health, expected_health) in [
        (ActorRole::Civilian, false, None),
        (ActorRole::Civilian, true, Some(10)),
        (ActorRole::Hostile, true, Some(9)),
    ] {
        let mut game = Game::empty();
        let target = game.place(Vec2::new(0.0, 4.0), passive(role, health)).unwrap();
        game.set_attack_profile(AttackProfile::Disintegrate);
        for tick in 1..=4 {
            step(&mut game, false, true);
            assert_eq!(impulses(&game, game.player_id(), target), tick);
            assert_eq!(game.health(target), expected_health);
        }
        assert!(game.body_pos(target).unwrap().z > 4.0);
    }
}

fn swipe_scene() -> GameScene {
    let attacker = ActorDefinition {
        actions: vec![ActionDefinition::Swipe(SwipeAction {
            timing: ActionTiming { startup: 2, active: 5, recovery: 10 },
            impulse: 0.1,
            ..Default::default()
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::attacker()
    };
    GameScene { engine: Scene { bodies: vec![
        Placed { pos: (0.0, 1.4), what: attacker },
        Placed { pos: (0.65, 0.65), what: passive(ActorRole::Civilian, true) },
    ], ..Default::default() }, ..Default::default() }
}

#[test]
fn autonomous_swipe_impacts_bystanders_once_and_only_damages_selected_target() {
    let setup = swipe_scene();
    let mut game = Game::from_scene(&setup).unwrap();
    let mut replay = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let actor = game.scene_bodies(owner).unwrap()[0];
    let bystander = game.scene_bodies(owner).unwrap()[1];
    for tick in 0..7 {
        step(&mut game, false, false);
        step(&mut replay, false, false);
        assert_eq!(game.hash(), replay.hash());
        assert_eq!(impulses(&game, actor, bystander), usize::from(tick >= 2), "tick {tick}");
        assert_eq!(impulses(&game, actor, game.player_id()), usize::from(tick >= 2), "tick {tick}");
        assert_eq!(game.health(bystander), Some(10));
        assert_eq!(game.player_health(), if tick < 2 { 5 } else { 4 });
    }
    game.restart().unwrap();
    assert_eq!(game.hash(), Game::from_scene(&setup).unwrap().hash());
}

#[test]
fn a_new_body_generation_can_be_struck_during_the_same_active_window() {
    let mut game = Game::from_scene(&swipe_scene()).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let actor = game.scene_bodies(owner).unwrap()[0];
    let old = game.scene_bodies(owner).unwrap()[1];
    for _ in 0..3 { step(&mut game, false, false); }
    assert_eq!(impulses(&game, actor, old), 1);
    assert!(game.despawn_body(old));
    let fresh = game.place(Vec2::new(0.65, 0.65), passive(ActorRole::Civilian, false)).unwrap();
    assert_ne!(old, fresh);
    step(&mut game, false, false);
    assert_eq!(impulses(&game, actor, fresh), 1);
    for _ in 0..3 { step(&mut game, false, false); }
    assert_eq!(impulses(&game, actor, fresh), 1);
    assert_eq!(impulses(&game, actor, old), 1);
    assert_eq!(game.health(fresh), None);
}

#[test]
fn rejected_attack_impulse_tuning_preserves_live_state_and_accepted_birth() {
    let mut game = Game::from_scene(&swipe_scene()).unwrap();
    let ticket = game.request_spawn(Vec2::new(10.0, 0.0), ActorDefinition::civilian()).unwrap();
    for invalid in [f32::NAN, f32::INFINITY, -1.0, f32::MAX] {
        let mut setup = swipe_scene();
        let ActionDefinition::Swipe(swipe) = &mut setup.engine.bodies[0].what.actions[0] else { unreachable!() };
        swipe.impulse = invalid;
        let before = game.hash();
        assert!(game.load_scene(&setup).is_err());
        assert_eq!(game.hash(), before);
    }
    step(&mut game, false, false);
    assert!(game.spawn_outcomes().iter().any(|outcome| outcome.ticket == ticket && outcome.entity.is_some()));
}

#[test]
fn zero_impulse_does_not_prevent_damage_to_a_fixed_target() {
    let attacker = ActorDefinition {
        actions: vec![ActionDefinition::Swipe(SwipeAction {
            target: arpg_game::TargetSelection::NearestHostile,
            timing: ActionTiming { startup: 1, active: 1, recovery: 10 },
            impulse: 0.0,
            ..Default::default()
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::civilian()
    };
    let target = ActorDefinition { physical: Template::BLOCK.with_health(NonZeroU8::new(3).unwrap()), ..ActorDefinition::passive_hostile() };
    let mut game = Game::from_scene(&GameScene { engine: Scene { bodies: vec![
        Placed { pos: (4.0, 4.0), what: attacker },
        Placed { pos: (4.0, 5.0), what: target },
    ], ..Default::default() }, ..Default::default() }).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let actor = game.scene_bodies(owner).unwrap()[0];
    let target = game.scene_bodies(owner).unwrap()[1];
    step(&mut game, false, false);
    assert_eq!(game.health(target), Some(3));
    step(&mut game, false, false);
    assert_eq!(game.health(target), Some(2));
    assert_eq!(game.motion(target).unwrap().velocity(), Vec2::ZERO);
    assert_eq!(impulses(&game, actor, target), 1);
}

#[test]
fn leap_landing_impacts_a_healthless_bystander_without_granting_damage_membership() {
    let attacker = ActorDefinition {
        actions: vec![ActionDefinition::Leap(arpg_game::LeapAction {
            timing: ActionTiming { startup: 2, active: 36, recovery: 10 },
            impulse: 0.5,
            ..Default::default()
        })],
        behavior: BehaviorNode::Action(0),
        ..ActorDefinition::attacker()
    };
    let mut game = Game::from_scene(&GameScene { engine: Scene { bodies: vec![
        Placed { pos: (0.0, 4.0), what: attacker },
        Placed { pos: (0.8, 0.0), what: passive(ActorRole::Civilian, false) },
    ], ..Default::default() }, ..Default::default() }).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let actor = game.scene_bodies(owner).unwrap()[0];
    let target = game.scene_bodies(owner).unwrap()[1];
    for tick in 0..48 {
        step(&mut game, false, false);
        assert_eq!(impulses(&game, actor, target), usize::from(tick >= 38), "tick {tick}");
        assert_eq!(game.health(target), None);
    }
}

#[test]
fn interaction_cover_follows_panel_pose_even_after_its_latch_is_released() {
    use arpg_game::{DefeatObjective, DoorSpec, EncounterAction, EncounterObjective, EncounterSpec, EncounterStart, SourceSpec};
    use arpg_sim::{Condition, Impulse, InteractionState, LockState};
    let door = ActorDefinition { physical: Template::BODY, ..ActorDefinition::prop() };
    let setup = GameScene {
        engine: Scene {
            bodies: vec![
                Placed { pos: (0.0, 0.5), what: door },
                Placed { pos: (0.0, 1.5), what: ActorDefinition::prop().interactive() },
                Placed { pos: (1.4, 0.0), what: ActorDefinition::prop().interactive() },
            ],
            sources: vec![SourceSpec { pos: (20.0, 20.0), radius: 0.0, every: 10_000,
                when: Condition::Always, what: ActorDefinition::passive_hostile(), enabled: false }],
            ..Default::default()
        },
        doors: vec![DoorSpec { body: 0, panel: DoorPanel::new(0.8, 0.18, 0.0, -1.55, 1.55, 12.0).unwrap() }],
        encounters: vec![EncounterSpec {
            name: "Release test".into(), start: EncounterStart::Activated(2),
            objective: EncounterObjective::Defeat(DefeatObjective { sources: vec![0], count: 1.try_into().unwrap(), within_ticks: 1000.try_into().unwrap() }),
            on_start: vec![EncounterAction::UnlockDoor(0)], on_success: vec![], on_failure: vec![],
        }],
        ..Default::default()
    };
    let mut game = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let door = game.scene_bodies(owner).unwrap()[0];
    let behind = game.scene_bodies(owner).unwrap()[1];
    let trigger = game.scene_bodies(owner).unwrap()[2];
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    let interact = Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_interact(true);
    game.step(dt, interact);
    assert_eq!(game.interaction_state(behind), Some(InteractionState::Ready));
    assert_eq!(game.interaction_state(trigger), Some(InteractionState::Activated));
    assert_eq!(game.joint(door).unwrap().lock(), LockState::Held);
    game.step(dt, interact);
    assert_eq!(game.joint(door).unwrap().lock(), LockState::Released);
    assert_eq!(game.joint(door).unwrap().angle(), 0.0);
    assert_eq!(game.interaction_state(behind), Some(InteractionState::Ready));
    assert!(game.apply_impulse(door, Impulse::try_from((0.0, 8.0)).unwrap()));
    for _ in 0..30 { step(&mut game, false, false); }
    assert!(game.joint(door).unwrap().angle().abs() > 1.0);
    game.step(dt, interact);
    assert_eq!(game.interaction_state(behind), Some(InteractionState::Activated));
}

fn panel_scene(centre: (f32, f32), width: f32) -> GameScene {
    GameScene {
        engine: Scene { bodies: vec![Placed { pos: centre, what: ActorDefinition {
            physical: Template::BODY, ..ActorDefinition::prop()
        } }], ..Default::default() },
        doors: vec![arpg_game::DoorSpec { body: 0, panel: arpg_game::DoorPanel::new(width, 0.18, 0.0, -1.55, 1.55, 10_000.0).unwrap() }],
        ..Default::default()
    }
}

#[test]
fn melee_contacts_the_panel_surface_even_when_its_centre_is_outside_the_hit_disc() {
    let mut game = Game::from_scene(&panel_scene((0.0, 2.0), 6.0)).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let panel = game.scene_bodies(owner).unwrap()[0];
    game.set_attack_profile(AttackProfile::Cleave);
    let timing = game.attack_status().resolved.unwrap();
    step(&mut game, true, false);
    for _ in 1..timing.startup() { step(&mut game, false, false); }
    assert_eq!(impulses(&game, game.player_id(), panel), 0);
    step(&mut game, false, false);
    assert_eq!(impulses(&game, game.player_id(), panel), 1,
        "first Cleave disc at (-1.8,1.2) reaches the panel's face but not its centre");
    for _ in 1..timing.active() { step(&mut game, false, false); }
    assert_eq!(impulses(&game, game.player_id(), panel), 1);
}

#[test]
fn beam_hits_a_panel_and_its_physical_cover_protects_bodies_behind_it() {
    let mut setup = panel_scene((0.0, 2.0), 4.0);
    setup.engine.bodies.push(Placed { pos: (0.0, 4.0), what: passive(ActorRole::Hostile, true) });
    let mut game = Game::from_scene(&setup).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let panel = game.scene_bodies(owner).unwrap()[0];
    let behind = game.scene_bodies(owner).unwrap()[1];
    game.set_attack_profile(AttackProfile::Disintegrate);
    for tick in 1..=13 {
        step(&mut game, false, true);
        assert_eq!(impulses(&game, game.player_id(), panel), tick);
        assert_eq!(impulses(&game, game.player_id(), behind), 0);
        assert_eq!(game.health(behind), Some(10));
        assert!((game.beam().unwrap().end().y - 1.91).abs() < 0.00001);
    }
}

#[test]
fn a_target_that_becomes_neutral_still_takes_the_committed_impact_but_no_damage() {
    let attacker = ActorDefinition {
        actions: vec![ActionDefinition::Swipe(SwipeAction {
            target: arpg_game::TargetSelection::NearestHostile,
            timing: ActionTiming { startup: 2, active: 2, recovery: 10 },
            impulse: 0.1,
            ..Default::default()
        })], behavior: BehaviorNode::Action(0), ..ActorDefinition::civilian()
    };
    let mut game = Game::from_scene(&GameScene { engine: Scene { bodies: vec![
        Placed { pos: (4.0, 4.0), what: attacker },
        Placed { pos: (4.0, 5.0), what: passive(ActorRole::Hostile, true) },
    ], ..Default::default() }, ..Default::default() }).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let actor = game.scene_bodies(owner).unwrap()[0];
    let target = game.scene_bodies(owner).unwrap()[1];
    step(&mut game, false, false);
    game.assign_behavior(target, passive(ActorRole::Civilian, true)).unwrap();
    for _ in 0..3 { step(&mut game, false, false); }
    assert_eq!(impulses(&game, actor, target), 1);
    assert_eq!(game.health(target), Some(10));
    assert!(game.motion(target).unwrap().velocity().y > 0.0);
}

#[test]
fn visual_connector_and_beam_share_the_nearest_panel_or_wall_hit() {
    let mut game = Game::from_scene(&panel_scene((0.0, 2.0), 4.0)).unwrap();
    let panel_owner = game.scene_instances().next().unwrap().0;
    game.load_scene(&GameScene { engine: Scene { walls: vec![
        arpg_sim::Wall::new(Vec2::new(0.0, 4.0), Vec2::new(4.0, 0.2), 0.0).unwrap(),
    ], ..Default::default() }, ..Default::default() }).unwrap();
    // A sword-to-beam connector is displaced from the player's centre ray.
    let start = Vec2::new(0.4, 0.4);
    let end = Vec2::new(-0.2, 6.0);
    let before = game.hash();
    let hit = game.solid_hit(start, end).unwrap().unwrap();
    assert!((hit.point().y - 1.91).abs() < 0.00001);
    assert_eq!(hit.normal(), Vec2::NEG_Y);
    let reverse = game.solid_hit(end, start).unwrap().unwrap();
    assert!((reverse.point().y - 4.1).abs() < 0.00001);
    assert_eq!(reverse.normal(), Vec2::Y);
    assert_eq!(game.hash(), before, "presentation queries cannot change physical state");
    game.set_attack_profile(AttackProfile::Disintegrate);
    step(&mut game, false, true);
    assert!((game.beam().unwrap().end().y - hit.point().y).abs() < 0.00001);
    assert!(game.evict_scene(panel_owner));
    let remaining = game.solid_hit(start, end).unwrap().unwrap();
    assert!((remaining.point().y - 3.9).abs() < 0.00001);
}
