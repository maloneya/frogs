//! Player facing policy is exercised through the ordinary Game schedule.
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::Game;
use arpg_sim::{Accumulator, Alpha, Dt};
use glam::Vec3;

fn step(game: &mut Game, aim: Vec3, attack: bool) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::new(aim).unwrap(), attack));
}

#[test]
fn ordinary_aim_turns_at_the_authored_rate_and_stops_on_target() {
    let mut game = Game::empty();
    for tick in 1..=60 {
        let before = game.player_facing();
        step(&mut game, Vec3::X, false);
        let expected = (14.0 * tick as f32 / 60.0).min(core::f32::consts::FRAC_PI_2);
        assert!((game.player_facing() - expected).abs() < 1e-5);
        assert_eq!(game.player_presentation(Alpha::ZERO).facing(), before);
        assert_eq!(game.player_presentation(Alpha::ONE).facing(), game.player_facing());
    }
    let settled = game.player_facing();
    step(&mut game, Vec3::X, false);
    assert_eq!(game.player_facing(), settled);
    assert_eq!(game.player_pos().x, 0.0, "aim does not request movement");
}

#[test]
fn exact_reverse_aim_uses_the_positive_half_turn_and_crosses_the_seam_by_the_short_arc() {
    let mut game = Game::empty();
    step(&mut game, Vec3::NEG_Z, false);
    assert!((game.player_facing() - 14.0 / 60.0).abs() < 1e-6);
    let before_seam = core::f32::consts::PI - 0.01;
    let after_seam = -core::f32::consts::PI + 0.01;
    let direction = |yaw: f32| Vec3::new(yaw.sin(), 0.0, yaw.cos());
    step(&mut game, direction(before_seam), true);
    let committed = game.player_facing();
    for _ in 1..22 {
        step(&mut game, direction(after_seam), false);
        assert_eq!(game.player_facing(), committed, "the swing owns facing until its final recovery tick");
    }
    step(&mut game, direction(after_seam), false);
    assert!((game.player_facing() - after_seam).abs() < 1e-6);
    assert_eq!(game.player_presentation(Alpha::ZERO).facing(), committed);
    assert!((game.player_presentation(Alpha::ONE).facing() - after_seam).abs() < 1e-6);
}
