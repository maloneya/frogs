//! Source admission shares one rule across direct creation and scene loading.

use arpg_game::{Scene, SourceSpec, Source, ActorDefinition};
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::Game;
use arpg_sim::{Accumulator, Condition, Dt, SourceState, };
use glam::Vec2;

fn spec() -> SourceSpec {
    SourceSpec { enabled: true, pos: (30.0, 0.0), radius: 0.0, every: 2,
        when: Condition::Always, what: ActorDefinition::passive_hostile() }
}

fn step(game: &mut Game) {
    for dt in Accumulator::default().pending(Dt::SECS) {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
    }
}

#[test]
fn rejected_sources_preserve_live_state_and_accepted_pending_work() {
    let mut game = Game::empty();
    let mut control = Game::empty();
    for run in [&mut game, &mut control] {
        let _ = run.add_source(Source::try_from(spec()).unwrap());
        step(run);
        assert!(run.request_spawn(Vec2::new(-20.0, 0.0), ActorDefinition::passive_hostile()).is_some());
    }
    let before = (game.hash(), game.trace().render());
    for invalid in [
        SourceSpec { pos: (f32::NAN, 0.0), ..spec() },
        SourceSpec { pos: (f32::INFINITY, 0.0), ..spec() },
        SourceSpec { radius: -1.0, ..spec() },
        SourceSpec { radius: f32::NAN, ..spec() },
        SourceSpec { radius: f32::INFINITY, ..spec() },
        SourceSpec { pos: (f32::MAX, 0.0), radius: f32::MAX, ..spec() },
        SourceSpec { when: Condition::PlayerWithin(-1.0), ..spec() },
        SourceSpec { when: Condition::PlayerWithin(f32::NAN), ..spec() },
        SourceSpec { when: Condition::PlayerWithin(f32::INFINITY), ..spec() },
        SourceSpec { pos: (96.0, 0.0), what: ActorDefinition::prop(), ..spec() },
    ] {
        assert!(Source::try_from(invalid.clone()).is_err());
        let scene = Scene { name: "refused".into(), sources: vec![invalid], ..Scene::default() };
        assert!(game.load_scene(&scene.into()).is_err());
        assert_eq!((game.hash(), game.trace().render()), before);
    }
    // Tick 1 drains the prior request while the existing source waits. Tick 2
    // emits again. Rejection must change neither reservation nor source cadence.
    for expected in [2, 3] {
        step(&mut game);
        step(&mut control);
        assert_eq!(game.enemy_count(), expected);
        assert_eq!(game.hash(), control.hash());
    }
}

#[test]
fn zero_radius_and_cadence_normalize_and_paused_sources_remain_ready() {
    let mut game = Game::empty();
    let id = game.add_source(Source::try_from(SourceSpec {
        every: 0, enabled: false, ..spec()
    }).unwrap());
    step(&mut game);
    assert_eq!(game.enemy_count(), 0);
    assert_eq!(game.source_state(id), Some(SourceState { enabled: false, countdown: 0, emitted: 0 }));
    assert!(game.set_source_enabled(id, true));
    for expected in [1, 2] {
        step(&mut game);
        assert_eq!(game.enemy_count(), expected, "normalized cadence emits each enabled tick");
    }
}
