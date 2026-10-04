//! A slam-speed impact carries through bodies it did not touch before the tick.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{ActorDefinition, Game, GameScene, Placed};
use arpg_sim::{Accumulator, Dt, Impulse, Template, TuningField};
use glam::Vec2;

fn setup() -> GameScene {
    let mut scene = GameScene::default();
    scene.engine.bodies = [0.0, 0.8, 1.4]
        .into_iter()
        .map(|x| Placed {
            pos: (x, 10.0),
            what: ActorDefinition {
                physical: Template::BODY,
                ..ActorDefinition::prop()
            },
        })
        .collect();
    scene
}

#[test]
fn momentum_transferred_to_an_idle_body_reaches_the_next_body_without_crossing_it() {
    let scene = setup();
    let mut game = Game::from_scene(&scene).unwrap();
    let mut replay = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let ids = game.scene_bodies(owner).unwrap().to_vec();
    // The default slam: 72 m/s on a 1kg body crosses the whole row in one tick
    // unless each newly struck body finds its next contact within the tick.
    let slam = game.tuning().get(TuningField::SlamImpulse);
    let impulse = Impulse::try_from((slam, 0.0)).unwrap();
    for game in [&mut game, &mut replay] {
        assert!(game.apply_impulse(ids[0], impulse));
    }
    let dt = Accumulator::default().pending(Dt::SECS).next().unwrap();
    let mut previous_energy = 0.5 * slam * slam;
    for tick in 0..3 {
        for game in [&mut game, &mut replay] {
            game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
        }
        assert_eq!(game.hash(), replay.hash());
        let positions: Vec<_> = ids.iter().map(|&id| game.body_pos(id).unwrap()).collect();
        for pair in positions.windows(2) {
            assert!(
                pair[1].x - pair[0].x >= 0.5 - 0.01,
                "tick {tick}: an impact crossed a body: {positions:?}"
            );
        }
        for position in &positions {
            assert!((position.z - 10.0).abs() < 1e-4);
        }
        let momentum = ids
            .iter()
            .map(|&id| game.motion(id).unwrap().velocity())
            .sum::<Vec2>();
        // Every undriven body receives the same .9 per-tick passive drag after
        // internal contacts, so internal transfers preserve this exact total.
        let expected = slam * 0.9_f32.powi(tick + 1);
        assert!(
            (momentum - Vec2::X * expected).length() < 0.001,
            "momentum {momentum:?}, expected {expected}"
        );
        let energy = ids
            .iter()
            .map(|&id| game.motion(id).unwrap().velocity().length_squared() * 0.5)
            .sum::<f32>();
        assert!(
            energy <= previous_energy + 1e-3,
            "contact manufactured energy: {energy} > {previous_energy}"
        );
        previous_energy = energy;
    }
    assert!(
        game.motion(ids[2]).unwrap().velocity().x > 0.0,
        "the third body received momentum"
    );
}
