//! Placement overlap relaxes through the shared soft contact response: bounded
//! by the push speed, conserving momentum, and ending at rest.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::{ActorDefinition, Game, Placed, Scene};
use arpg_sim::{Accumulator, Dt, EntityId, TuningField};
use glam::Vec2;

fn step(game: &mut Game) {
    game.step(
        Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false),
    );
}
fn positions(game: &Game, ids: &[EntityId]) -> Vec<Vec2> {
    ids.iter()
        .map(|&id| {
            let p = game.body_pos(id).unwrap();
            Vec2::new(p.x, p.z)
        })
        .collect()
}

#[test]
fn a_birth_inside_the_player_is_pushed_out_at_bounded_speed_without_velocity() {
    for queued in [false, true] {
        let mut game = Game::empty();
        let (direct, ticket) = if queued {
            (
                None,
                Some(
                    game.request_spawn(Vec2::new(0.0, 0.2), ActorDefinition::prop())
                        .unwrap(),
                ),
            )
        } else {
            (
                Some(
                    game.place(Vec2::new(0.0, 0.2), ActorDefinition::prop())
                        .unwrap(),
                ),
                None,
            )
        };
        step(&mut game);
        let id = direct.unwrap_or_else(|| {
            game.spawn_outcomes()
                .iter()
                .find(|outcome| Some(outcome.ticket) == ticket)
                .unwrap()
                .entity
                .unwrap()
        });
        let push = game.tuning().get(TuningField::ContactPushSpeed);
        let ground = |game: &Game, id| {
            let p = game.body_pos(id).unwrap();
            Vec2::new(p.x, p.z)
        };
        let mut previous = 0.2;
        for tick in 0..60 {
            if tick > 0 {
                step(&mut game);
            }
            let player = game.player_pos();
            let distance = ground(&game, id).distance(Vec2::new(player.x, player.z));
            assert!(
                distance - previous <= push * Dt::SECS + 1e-5,
                "tick {tick}: separation outran the push speed"
            );
            previous = distance;
            for body in [id, game.player_id()] {
                assert!(
                    game.motion(body).unwrap().velocity().length() < 1e-3,
                    "tick {tick}: overlap correction left velocity behind"
                );
            }
        }
        assert!(
            previous >= 0.55 - 0.0005 - 1e-5,
            "the pair ends at the contact slop"
        );
    }
}

#[test]
fn coincident_and_cross_cell_clusters_settle_without_energy() {
    for initial in [
        vec![(20.0, 0.0); 5],
        vec![(10.49, 3.0), (10.48, 3.0), (11.01, 3.0)],
    ] {
        let scene = Scene {
            bodies: initial
                .iter()
                .map(|&pos| Placed {
                    pos,
                    what: ActorDefinition::passive_hostile(),
                })
                .collect(),
            ..Default::default()
        }
        .into();
        let mut game = Game::from_scene(&scene).unwrap();
        let mut replay = Game::from_scene(&scene).unwrap();
        let ids = game
            .scene_bodies(game.scene_instances().next().unwrap().0)
            .unwrap()
            .to_vec();
        let centre = initial.iter().map(|&p| Vec2::from(p)).sum::<Vec2>() / initial.len() as f32;
        let mut settled = vec![];
        let mut speed = 0.0_f32;
        for tick in 0..240 {
            step(&mut game);
            step(&mut replay);
            assert_eq!(game.hash(), replay.hash());
            assert!(game.all_positions_finite());
            let current = positions(&game, &ids);
            assert!(
                (current.iter().copied().sum::<Vec2>() / ids.len() as f32).distance(centre)
                    < 0.0001,
                "internal correction preserves the equal-mass centre"
            );
            for (&id, &position) in ids.iter().zip(&current) {
                speed = speed.max(game.motion(id).unwrap().velocity().length());
                assert!(
                    position.distance(centre) < 1.5,
                    "correction must not explode the cluster"
                );
            }
            if tick == 179 {
                settled = current.clone();
            }
            if tick >= 180 {
                assert_eq!(current, settled, "the cluster must come to rest");
            }
        }
        // A single relaxation sweep cannot fully cancel coupled push-outs in
        // a pile, so bodies drift apart below the push speed, then drag stops them.
        assert!(
            speed < game.tuning().get(TuningField::ContactPushSpeed),
            "{speed}"
        );
        for (i, &a) in settled.iter().enumerate() {
            for &b in &settled[i + 1..] {
                assert!(a.distance(b) >= 0.5 - 0.0005 - 1e-5);
            }
        }
        assert_eq!(game.enemy_count(), initial.len());
    }
}
