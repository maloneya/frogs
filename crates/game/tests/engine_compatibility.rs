//! Compatibility at the adapter boundary: the game must preserve the engine's
//! tick history and physical output. The restart report is the explicit new
//! game-owned state; the engine fingerprint remains unchanged by that addition.

use arpg_core::{InstanceBuffer, Intent, MoveDir, Report};
use arpg_game::Game;
use arpg_sim::{
    Accumulator, Alpha, AttackProfile, Condition, Dt, Event, Impulse, Placed,
    Scene, SourceSpec, Template, World,
};
use glam::{Vec2, Vec3};

fn assert_same(game: &Game, world: &World, restart: Option<(&str, u64)>) {
    assert_eq!(game.tick(), world.tick());
    assert_eq!(game.engine_hash(), world.hash(), "state differs at tick {}", game.tick());
    assert_eq!(game.trace().render(), world.trace().render());
    let mut game_report = Report::default();
    let mut engine_report = Report::default();
    game.report(&mut game_report);
    world.report(&mut engine_report);
    engine_report.object("source_controls", |_| {});
    engine_report.int("control_events_dropped", 0);
    engine_report.object("restart", |out| {
        out.bool("available", restart.is_some());
        if let Some((name, initial_hash)) = restart {
            out.text("name", name);
            out.text("initial_hash", &format!("{initial_hash:016x}"));
        }
    });
    assert_eq!(game_report.finish(), engine_report.finish());
}

#[test]
fn construction_preserves_existing_identity_history() {
    assert_same(&Game::default(), &World::default(), None);
    assert_same(&Game::empty(), &World::empty(), None);
    let scene = Scene::boot();
    let world = World::from_scene(&scene).unwrap();
    assert_same(&Game::from_scene(&scene.clone().into()).unwrap(), &world, Some((&scene.name, world.hash())));
}

#[test]
fn mixed_inputs_preserve_ticks_trace_reports_and_interpolation() {
    let scene = Scene {
        name: "entry point compatibility".into(),
        bodies: vec![
            Placed { pos: (1.0, 0.0), what: Template::BLOCK.interactive() },
            Placed { pos: (0.0, 1.0), what: Template::BODY },
        ],
        sources: vec![SourceSpec {
            enabled: true,
            pos: (4.0, 4.0), radius: 1.0, every: 12,
            when: Condition::FewerThan(8), what: Template::BODY.seeking(),
        }],
        grids: vec![],
    };
    let mut game = Game::from_scene(&scene.clone().into()).unwrap();
    let mut world = World::from_scene(&scene).unwrap();
    let restart = Some((scene.name.as_str(), world.hash()));
    assert_same(&game, &world, restart);
    let mut clock = Accumulator::default();
    let mut drawn_game = InstanceBuffer::default();
    let mut drawn_engine = InstanceBuffer::default();
    let mut stepped = 0;

    // Includes frames with no tick and frames with several. Both adapters must
    // preserve the same fixed-tick boundary and interpolation remainder.
    for frame in 0..40 {
        let elapsed = [0.25, 0.25, 1.5, 3.0][frame % 4] * Dt::SECS;
        for dt in clock.pending(elapsed) {
            if stepped == 8 {
                let impulse = Impulse::try_from((2.0, -1.0)).unwrap();
                assert!(game.apply_impulse(game.player_id(), impulse));
                assert!(world.apply_impulse(world.player_id(), impulse));
                assert!(game.request_spawn(Vec2::new(-3.0, 2.0), Template::BODY));
                assert!(world.request_spawn(Vec2::new(-3.0, 2.0), Template::BODY));
            }
            if stepped == 20 {
                game.set_attack_profile(AttackProfile::Sweep);
                world.set_attack_profile(AttackProfile::Sweep);
            }
            let direction = if stepped < 12 { Vec3::ZERO } else { Vec3::Z };
            let intent = Intent::new(MoveDir::new(direction), stepped % 24 == 0)
                .with_interact(stepped == 0);
            game.step(dt, intent);
            world.step(dt, intent);
            stepped += 1;
            assert_same(&game, &world, restart);
        }

        let before = game.hash();
        for alpha in [Alpha::ZERO, clock.alpha(), Alpha::ONE] {
            assert_eq!(game.player_pos_at(alpha), world.player_pos_at(alpha));
            game.extract(alpha, drawn_game.sink());
            world.extract(alpha, drawn_engine.sink());
            let actual = drawn_game.as_slice();
            let expected = drawn_engine.as_slice();
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(expected) {
                assert_eq!(actual.pos(), expected.pos());
                assert_eq!(actual.yaw(), expected.yaw());
            }
        }
        assert_eq!(game.hash(), before, "presentation must not change playable state");
    }

    assert!(stepped > 24, "exercise both swings and the tuning change");
    assert!(game.trace().iter().any(|(_, event)| matches!(event, Event::Activated { .. })));
    assert!(game.trace().iter().any(|(_, event)| matches!(event, Event::Fired { .. })));
    assert!(game.enemy_count() > 1, "the source and queued request must actually spawn");
}
