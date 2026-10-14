//! A tree's root fails like a door latch; the fall, what it meets and the log
//! it leaves are physics, and damage from an impact follows only from health.
use arpg_core::{AimDir, Intent, MoveDir, Report};
use arpg_game::{ActorDefinition, Game, GameScene, TreeKind, TreeSpec, TreeState};
use arpg_sim::{Accumulator, Dt, EntityId, Event, Impulse, LockState};
use glam::Vec2;

/// Unit-scale pine: 4.35 m tall, 0.4 m base, 1.25 m crown, 300 kg, 160 kg m/s root.
const HEIGHT: f32 = 4.35;
const WIDTH: f32 = 0.4;
const ROOT: f32 = 160.0;
const INVERSE_INERTIA: f32 = 3.0 / (300.0 * HEIGHT * HEIGHT);
/// Trees count pushes, and bodies' height, at one metre.
const LOAD_HEIGHT: f32 = 1.0;

fn pine(at: Vec2) -> TreeSpec { TreeSpec::new(TreeKind::Pine, at, 0.0, 1.0).unwrap() }
fn grove(trees: Vec<TreeSpec>) -> GameScene { GameScene { trees, ..GameScene::default() } }
fn step(game: &mut Game) {
    game.step(Accumulator::default().pending(Dt::SECS).next().unwrap(), Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
}
fn tree(game: &Game, nth: usize) -> EntityId {
    let owner = game.scene_instances().next().unwrap().0;
    game.tree_body(owner, nth).unwrap()
}
fn state(game: &Game, nth: usize) -> TreeState {
    let owner = game.scene_instances().next().unwrap().0;
    game.tree_state(owner, nth).unwrap()
}
fn shove(game: &mut Game, body: EntityId, amount: f32) {
    let base = game.joint(body).unwrap().anchor();
    assert!(game.apply_impulse_at(body, base, Impulse::try_from((0.0, amount)).unwrap()));
}
fn ticks_of(game: &Game, wanted: impl Fn(Event) -> bool) -> Vec<u64> {
    game.trace().iter().filter(|(_, event)| wanted(*event)).map(|(tick, _)| tick).collect()
}
/// Steps until the trunk lands, returning the landing tick.
fn until_down(game: &mut Game, body: EntityId, limit: u64) -> u64 {
    while !game.joint(body).unwrap().grounded() {
        assert!(game.tick() < limit, "trunk still standing or leaning at tick {}", game.tick());
        step(game);
    }
    game.tick() - 1
}

#[test]
fn a_root_spends_its_strength_and_the_excess_starts_the_fall() {
    let mut game = Game::from_scene(&grove(vec![pine(Vec2::new(0.0, 5.0))])).unwrap();
    let body = tree(&game, 0);
    // Below strength the root holds and reports the whole tick's load.
    shove(&mut game, body, 150.0);
    step(&mut game);
    let trunk = game.joint(body).unwrap();
    assert_eq!(trunk.lock(), LockState::Held);
    assert!((trunk.reaction_impulse() - 150.0).abs() < 1e-3);
    step(&mut game);
    assert_eq!(game.joint(body).unwrap().reaction_impulse(), 0.0, "load does not accumulate across ticks");

    // 200 kg m/s spends the 160 root; the 40 left over tips the trunk this tick.
    shove(&mut game, body, 200.0);
    step(&mut game);
    let trunk = game.joint(body).unwrap();
    assert_eq!(state(&game, 0), TreeState::Falling);
    assert_eq!(trunk.reaction_impulse(), ROOT);
    assert_eq!(trunk.direction(), Vec2::Y);
    let start_rate = 40.0 * LOAD_HEIGHT * INVERSE_INERTIA;
    assert!((trunk.rate() - start_rate).abs() < start_rate * 0.02, "{}", trunk.rate());
    assert_eq!(ticks_of(&game, |e| matches!(e, Event::LockBroken { id } if id == body)), [2]);

    // Unloaded, it falls as a rod about its base: rate² = start² + 3g(1 - cos)/H.
    while game.joint(body).unwrap().angle() < 1.0 {
        step(&mut game);
    }
    let trunk = game.joint(body).unwrap();
    let expected = (start_rate * start_rate + 3.0 * 9.81 * (1.0 - trunk.angle().cos()) / HEIGHT).sqrt();
    assert!((trunk.rate() - expected).abs() < expected * 0.01, "{} vs {expected}", trunk.rate());

    let landed = until_down(&mut game, body, 400);
    assert_eq!(ticks_of(&game, |e| matches!(e, Event::Toppled { id } if id == body)), [landed]);
    let log = game.joint(body).unwrap().collider();
    assert!(log.centre().abs_diff_eq(Vec2::new(0.0, 5.0 + (HEIGHT - 0.5 * WIDTH) * 0.5), 1e-4), "{}", log.centre());
    assert!(log.size().abs_diff_eq(Vec2::new(WIDTH, HEIGHT + 0.5 * WIDTH), 1e-4));
    // The log is solid: it blocks a walk across it, and stays down.
    let crossing = game.solid_hit(Vec2::new(-3.0, 8.0), Vec2::new(3.0, 8.0)).unwrap().unwrap();
    assert!((crossing.point().x + 0.5 * WIDTH).abs() < 1e-4);
    for _ in 0..30 { step(&mut game); }
    assert_eq!(state(&game, 0), TreeState::Down);
}

#[test]
fn a_fall_strikes_what_it_comes_down_on_once_and_damage_follows_health() {
    let mut game = Game::from_scene(&grove(vec![pine(Vec2::new(0.0, 5.0))])).unwrap();
    let body = tree(&game, 0);
    let victim = game.place(Vec2::new(0.0, 7.5), ActorDefinition::passive_hostile()).unwrap();
    let behind = game.place(Vec2::new(0.0, 3.5), ActorDefinition::passive_hostile()).unwrap();
    let aside = game.place(Vec2::new(1.6, 7.5), ActorDefinition::passive_hostile()).unwrap();
    let crate_ = game.place(Vec2::new(0.6, 9.0), ActorDefinition::default()).unwrap();
    shove(&mut game, body, 200.0);
    let mut seen = Vec::new();
    while !game.joint(body).unwrap().grounded() {
        step(&mut game);
        let trunk = game.joint(body).unwrap();
        for strike in game.impacts() {
            // Met where the trunk is down to body height over the body's near edge.
            let near = strike.point().y - 5.0 - 0.25;
            assert!(near * trunk.angle().cos() <= LOAD_HEIGHT * trunk.angle().sin() + 1e-4);
            seen.push((game.tick() - 1, strike.body(), *strike));
        }
        assert!(game.tick() < 400);
    }
    let bodies: Vec<_> = seen.iter().map(|(_, id, _)| *id).collect();
    assert_eq!(bodies, [victim, crate_], "behind and beyond the crown are untouched; each struck once");
    let (_, _, hit) = seen[0];
    // Closing speed is the trunk's horizontal speed at the load height. A 1 kg
    // body against a 300 kg trunk takes almost exactly that as momentum.
    assert!(hit.closing_speed() > 1.0 && hit.damaged());
    let effective = 1.0 / (1.0 + LOAD_HEIGHT * LOAD_HEIGHT * INVERSE_INERTIA);
    assert!((hit.impulse() - effective * hit.closing_speed()).abs() < 1e-4);
    // Three health, three damage: the victim is defeated by the trunk.
    assert!(game.trace().iter().any(|(_, e)| e == Event::Damaged { actor: body, target: victim, remaining: 0 }));
    assert!(game.health(victim).is_none(), "defeated bodies are removed");
    // A body without health is pushed along the fall but never damaged.
    let (_, _, pushed) = seen[1];
    assert!(pushed.impulse() > 0.0 && !pushed.damaged());
    assert!(game.motion(crate_).unwrap().velocity().y > 0.0 || game.body_pos(crate_).unwrap().z > 9.0);
    assert_eq!(game.health(behind), Some(3));
    assert_eq!(game.health(aside), Some(3));
}

#[test]
fn the_player_is_struck_like_anything_else() {
    let mut game = Game::from_scene(&grove(vec![pine(Vec2::new(0.0, -2.0))])).unwrap();
    let body = tree(&game, 0);
    let player = game.player_id();
    assert_eq!(game.health(player), Some(5));
    shove(&mut game, body, 260.0);
    until_down(&mut game, body, 400);
    assert!(game.trace().iter().any(|(_, e)| e == Event::Damaged { actor: body, target: player, remaining: 2 }));
    assert_eq!(game.health(player), Some(2));
}

#[test]
fn a_wall_holds_a_falling_trunk_leaning_above_the_bodies_under_it() {
    let mut scene = grove(vec![pine(Vec2::new(0.0, 5.0))]);
    scene.engine.walls.push(arpg_sim::Wall::new(Vec2::new(0.0, 7.2), Vec2::new(4.0, 0.4), 0.0).unwrap());
    let mut game = Game::from_scene(&scene).unwrap();
    let body = tree(&game, 0);
    let under = game.place(Vec2::new(0.0, 6.5), ActorDefinition::passive_hostile()).unwrap();
    let beyond = game.place(Vec2::new(0.0, 8.0), ActorDefinition::passive_hostile()).unwrap();
    shove(&mut game, body, 200.0);
    // The span beyond the base meets the wall face at z = 7: 5 + 4.35 sin(tilt) = 7.
    let lean = (2.0 / HEIGHT).asin();
    for _ in 0..400 { step(&mut game); }
    for _ in 0..30 {
        step(&mut game);
        let trunk = game.joint(body).unwrap();
        assert_eq!(state(&game, 0), TreeState::Falling);
        assert!((trunk.angle() - lean).abs() < 1e-3, "{} vs {lean}", trunk.angle());
        assert_eq!(trunk.rate(), 0.0);
        assert!(game.impacts().is_empty());
    }
    // At 1.25 m out the leaning trunk is still 1.1 m up: above body height.
    assert_eq!(game.health(under), Some(3));
    assert_eq!(game.health(beyond), Some(3));
}

#[test]
fn a_blocked_fall_forgets_despawned_generations_but_keeps_live_meetings() {
    let mut scene = grove(vec![pine(Vec2::new(0.0, 5.0))]);
    scene.engine.walls.push(arpg_sim::Wall::new(Vec2::new(0.0, 7.2), Vec2::new(4.0, 0.4), 0.0).unwrap());
    let mut game = Game::from_scene(&scene).unwrap();
    let trunk = tree(&game, 0);
    shove(&mut game, trunk, 200.0);
    for _ in 0..400 { step(&mut game); }
    let assert_met = |game: &Game, expected: usize| {
        let mut report = Report::default();
        game.report(&mut report);
        assert!(report.finish().contains(&format!("\"met\":{expected},")), "one tree retains only live meetings");
    };
    // Near the base, the blocked trunk is below contact height. A body can be
    // met even though the settled trunk has no closing speed and deals no damage.
    let keeper = game.place(Vec2::new(-0.35, 5.65), ActorDefinition::passive_hostile()).unwrap();
    step(&mut game);
    assert_eq!(game.impacts().iter().map(|hit| hit.body()).collect::<Vec<_>>(), [keeper]);
    assert_met(&game, 1);
    for _ in 0..32 {
        let fresh = game.place(Vec2::new(0.35, 5.65), ActorDefinition::passive_hostile()).unwrap();
        step(&mut game);
        assert_eq!(game.impacts().iter().map(|hit| (hit.body(), hit.damaged())).collect::<Vec<_>>(), [(fresh, false)]);
        assert_eq!(game.health(fresh), Some(3));
        assert_met(&game, 2);
        assert!(game.despawn_body(fresh));
        assert_met(&game, 1);
        let hash = game.hash();
        assert!(!game.despawn_body(fresh), "stale identity cannot revoke a live meeting");
        assert_eq!(game.hash(), hash);
        step(&mut game);
        assert!(game.impacts().is_empty(), "the surviving body is still met only once");
        assert_eq!(game.health(keeper), Some(3));
        assert_eq!(state(&game, 0), TreeState::Falling);
    }
    assert!(game.despawn_body(keeper));
    assert_met(&game, 0);
}

#[test]
fn a_falling_trunk_breaks_the_root_it_lands_against() {
    let mut game = Game::from_scene(&grove(vec![pine(Vec2::new(0.0, 5.0)), pine(Vec2::new(0.0, 8.0))])).unwrap();
    let (first, second) = (tree(&game, 0), tree(&game, 1));
    shove(&mut game, first, 200.0);
    until_down(&mut game, first, 600);
    until_down(&mut game, second, 600);
    let broke = |id| ticks_of(&game, move |e| e == Event::LockBroken { id });
    let landed = |id| ticks_of(&game, move |e| e == Event::Toppled { id });
    // Stopping the first trunk's rotation loads the second at the load height,
    // far beyond its root strength, so it falls away along the same line.
    assert!(broke(first)[0] < broke(second)[0]);
    assert!(broke(second)[0] < landed(second)[0] && landed(second)[0] <= landed(first)[0]);
    assert!(game.joint(second).unwrap().direction().abs_diff_eq(Vec2::Y, 1e-3));
}

#[test]
fn restart_and_eviction_retire_a_fall_in_progress() {
    let authored = grove(vec![pine(Vec2::new(0.0, 5.0))]);
    let fresh = {
        let game = Game::from_scene(&authored).unwrap();
        let mut report = Report::default();
        game.report(&mut report);
        (game.hash(), report.finish())
    };
    let mut game = Game::from_scene(&authored).unwrap();
    let body = tree(&game, 0);
    shove(&mut game, body, 400.0);
    for _ in 0..40 { step(&mut game); }
    assert_eq!(state(&game, 0), TreeState::Falling);
    game.restart().unwrap();
    let mut report = Report::default();
    game.report(&mut report);
    assert_eq!((game.hash(), report.finish()), fresh);
    assert_eq!(state(&game, 0), TreeState::Standing);

    let body = tree(&game, 0);
    shove(&mut game, body, 400.0);
    for _ in 0..40 { step(&mut game); }
    let owner = game.scene_instances().next().unwrap().0;
    assert!(game.evict_scene(owner));
    assert!(game.joint(body).is_none());
    assert!(!game.apply_impulse(body, Impulse::try_from((0.0, 1.0)).unwrap()), "stale trunk refuses impulses");
    for _ in 0..200 {
        step(&mut game);
        assert!(game.impacts().is_empty());
    }
    assert_eq!(game.tree_poses(arpg_sim::Alpha::ONE).count(), 0);
}

#[test]
fn presentation_tilts_the_mesh_toward_the_fall_without_changing_the_game() {
    let mut game = Game::from_scene(&grove(vec![TreeSpec::new(TreeKind::Bare, Vec2::new(3.0, 0.0), 0.7, 1.0).unwrap()])).unwrap();
    let pose = game.tree_poses(arpg_sim::Alpha::ONE).next().unwrap();
    assert_eq!((pose.yaw, pose.pitch), (game.trees().next().unwrap().yaw(), 0.0));
    let body = tree(&game, 0);
    assert!(game.apply_impulse(body, Impulse::try_from((-400.0, 0.0)).unwrap()));
    for _ in 0..30 { step(&mut game); }
    let hash = game.hash();
    let trunk = game.joint(body).unwrap();
    let [earlier, now] = [arpg_sim::Alpha::ZERO, arpg_sim::Alpha::ONE].map(|alpha| game.tree_poses(alpha).next().unwrap());
    assert!((now.yaw + core::f32::consts::FRAC_PI_2).abs() < 1e-5, "facing -X: {}", now.yaw);
    assert_eq!(now.pitch, trunk.angle());
    assert!(earlier.pitch > 0.0 && earlier.pitch < now.pitch, "interpolates from the previous tick");
    assert_eq!(game.hash(), hash);
}
