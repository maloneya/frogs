//! Gameplay relationships through the same public boundary used by adapters.
use arpg_core::{Intent, Report};
use arpg_game::{ControlPhase, Game, GameScene, SourceControlSpec};
use arpg_sim::{Accumulator, Dt, Placed, Scene, SourceSpec, Condition, Template};
use glam::Vec2;

fn scene() -> GameScene {
    GameScene {
        engine: Scene {
            name: "control trial".into(),
            bodies: vec![Placed { pos: (1.0, 0.0), what: Template::BLOCK.interactive() }],
            sources: vec![SourceSpec { pos: (20.0, 0.0), radius: 2.0, every: 4,
                when: Condition::Always, what: Template::BODY, enabled: false }],
            grids: vec![],
        },
        source_controls: vec![SourceControlSpec { body: 0, source: 0 }],
    }
}

fn step(game: &mut Game, interact: bool) {
    for dt in Accumulator::default().pending(Dt::SECS) {
        game.step(dt, Intent::NONE.with_interact(interact));
    }
}

fn observed(game: &Game) -> (u64, String, String) {
    let mut report = Report::default();
    game.report(&mut report);
    (game.hash(), report.finish(), game.render_trace_since(0))
}

#[test]
fn invalid_relationships_leave_live_state_pending_work_and_restart_untouched() {
    let authored = scene();
    let mut game = Game::from_scene(&authored).unwrap();
    step(&mut game, true);
    assert!(game.request_spawn(Vec2::new(40.0, 0.0), Template::BODY));
    let before = observed(&game);
    let mut bad_body = scene();
    bad_body.source_controls[0].body = 1;
    let mut bad_source = scene();
    bad_source.source_controls[0].source = 1;
    let mut not_interactable = scene();
    not_interactable.engine.bodies[0].what = Template::BLOCK;
    let mut enabled = scene();
    enabled.engine.sources[0].enabled = true;
    let mut duplicate = scene();
    duplicate.source_controls.push(duplicate.source_controls[0]);
    let mut invalid_engine = scene();
    invalid_engine.engine.sources[0].pos.0 = f32::NAN;
    for invalid in [bad_body, bad_source, not_interactable, enabled, duplicate, invalid_engine] {
        assert!(game.load_scene(&invalid).is_err());
        assert!(game.start_scene(&invalid).is_err());
        assert_eq!(observed(&game), before);
    }
    step(&mut game, false);
    assert_eq!(game.enemy_count(), 2, "queued work and the pending activation survive rejection");
    game.restart().unwrap();
    assert_eq!(observed(&game), observed(&Game::from_scene(&authored).unwrap()));
}

#[test]
fn activation_restart_eviction_and_independent_instances_share_one_lifecycle() {
    let authored = scene();
    let mut game = Game::from_scene(&authored).unwrap();
    let a = game.scene_instances().next().unwrap().0;
    let b = game.load_scene(&authored).unwrap();
    step(&mut game, true); // nearest tie goes to A's stable body identity
    assert_eq!(game.enemy_count(), 0);
    assert_eq!(game.source_control_state(a, 0).unwrap().phase, ControlPhase::Pending);
    step(&mut game, false);
    assert_eq!(game.enemy_count(), 1);
    assert_eq!(game.source_control_state(a, 0).unwrap().phase, ControlPhase::Started);
    assert_eq!(game.source_control_state(b, 0).unwrap().phase, ControlPhase::Pending);
    let source = game.scene_sources(a).unwrap()[0];
    assert!(game.set_source_enabled(source, false));
    step(&mut game, true); // consumes B's interaction; A stays consumed
    step(&mut game, false);
    assert!(!game.source_state(source).unwrap().enabled);
    assert_eq!(game.enemy_count(), 2);
    assert!(game.evict_scene(a));
    assert_eq!(game.source_control_state(a, 0), None);
    assert_eq!(game.source_control_state(b, 0).unwrap().phase, ControlPhase::Started);
    assert_eq!(game.enemy_count(), 1);
    // Restart after activation, removal, additive content and trace transitions.
    game.restart().unwrap();
    assert_eq!(observed(&game), observed(&Game::from_scene(&authored).unwrap()));
    step(&mut game, true);
    assert_eq!(game.enemy_count(), 0);
    // Restart also discards an activation which has not yet been consumed.
    game.restart().unwrap();
    step(&mut game, false);
    step(&mut game, false);
    assert_eq!(game.enemy_count(), 0);
}

#[test]
fn a_recycled_body_and_a_new_source_cannot_inherit_a_pending_connection() {
    for remove_body in [true, false] {
        let mut game = Game::from_scene(&scene()).unwrap();
        let owner = game.scene_instances().next().unwrap().0;
        let body = game.scene_bodies(owner).unwrap()[0];
        let source = game.scene_sources(owner).unwrap()[0];
        step(&mut game, true);
        if remove_body {
            assert!(game.despawn_body(body));
            let new = game.place(Vec2::new(1.0, 0.0), Template::BLOCK.interactive()).unwrap();
            assert_ne!(new, body);
        } else {
            assert!(game.remove_source(source));
            let new = game.add_source(scene().engine.sources[0].into());
            assert_ne!(new, source);
        }
        step(&mut game, true);
        step(&mut game, true);
        assert_eq!(game.source_control_state(owner, 0).unwrap().phase, ControlPhase::Orphaned);
        assert_eq!(game.enemy_count(), 0);
        assert_eq!(game.control_trace().iter().count(), 1);
    }
}

#[test]
fn relationship_identity_phase_and_restart_effect_participate_in_hashing() {
    let mut authored = scene();
    authored.engine.bodies.push(Placed { pos: (-1.0, 0.0), what: Template::BLOCK.interactive() });
    let mut other = authored.clone();
    other.source_controls[0].body = 1;
    let mut a = Game::from_scene(&authored).unwrap();
    let b = Game::from_scene(&other).unwrap();
    assert_eq!(a.engine_hash(), b.engine_hash());
    assert_ne!(a.hash(), b.hash(), "relationship endpoint must reach the hash");
    let mut engine_only = Game::from_scene(&authored.engine.clone().into()).unwrap();
    step(&mut a, true);
    step(&mut engine_only, true);
    assert_eq!(a.engine_hash(), engine_only.engine_hash());
    assert_ne!(a.hash(), engine_only.hash());
    let id = a.scene_instances().next().unwrap().0;
    assert!(a.evict_scene(id));
    let mut b = b;
    let id = b.scene_instances().next().unwrap().0;
    assert!(b.evict_scene(id));
    assert_eq!(a.source_count(), 0);
    assert_ne!(a.hash(), b.hash(), "cached connection must affect restart even after eviction");
}
