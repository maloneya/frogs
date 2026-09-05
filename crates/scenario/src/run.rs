//! Running one scenario against the simulation.

use arpg_core::MoveDir;
use arpg_sim::{Accumulator, Dt, World};
use glam::Vec3;

use crate::spec::{Expect, Scenario};

/// Everything one run produced. Kept separate from the checking so that the
/// determinism pass can compare two runs without re-deciding what "passed"
/// means.
pub(crate) struct Run {
    pub(crate) world: World,
    /// The world's hash after every tick, in order. This is what makes a
    /// divergence report a tick number instead of a shrug.
    pub(crate) hashes: Vec<u64>,
}

/// A single failed expectation, phrased so the message is the whole diagnosis.
pub(crate) struct Failure {
    pub(crate) what: String,
    pub(crate) expected: String,
    pub(crate) actual: String,
    /// Anything that turns "wrong number" into "wrong number, and here is the
    /// shape of the wrongness". Empty when there is nothing useful to add.
    pub(crate) note: String,
}

impl Failure {
    fn new(what: &str, expected: String, actual: String) -> Self {
        Self { what: what.into(), expected, actual, note: String::new() }
    }

    fn with_note(mut self, note: String) -> Self {
        self.note = note;
        self
    }
}

/// One tick of walking, in world units — `PLAYER_SPEED * Dt::SECS`.
///
/// Used only to phrase error messages, and only as a *unit*, never as a
/// diagnosis. An earlier version said "which is 2 ticks of walking — suspect
/// the input schedule", and the first mutation that reached it was a wall clamp
/// off by `PLAYER_RADIUS`, which is 0.3, which is coincidentally exactly two
/// ticks. The hint was confident and wrong, which is worse than no hint: it
/// spends the reader's attention pointing away from the cause. So this reports
/// the size of the error in the unit that makes it comparable, and stops there.
const TICK_OF_WALKING: f32 = 9.0 / 60.0;

/// Runs the scenario to its budget.
///
/// Deliberately mints its `Dt` from a real [`Accumulator`], one tick's worth at
/// a time, rather than reaching for some test-only shortcut: the whole claim a
/// scenario makes is about the code path the game runs, and a runner that
/// stepped the world by some other route would be testing a different program.
pub(crate) fn run(scenario: &Scenario) -> Run {
    let mut world = World::default();
    world.set_enemy_count(scenario.setup.enemies);

    let budget = scenario.budget.ticks as usize;
    let schedule = input_schedule(scenario);

    let mut accumulator = Accumulator::default();
    let mut hashes = Vec::with_capacity(budget);

    while hashes.len() < budget {
        for dt in accumulator.pending(Dt::SECS) {
            let dir = schedule[hashes.len()];
            world.step(dt, dir);
            hashes.push(world.hash());
        }
    }

    Run { world, hashes }
}

/// Flattens the input spans into one direction per tick.
///
/// Overlapping spans resolve last-wins, which is the rule that makes a scenario
/// readable top to bottom: a later line overrides an earlier one, the way a
/// later assignment does. Ticks no span covers get no input.
fn input_schedule(scenario: &Scenario) -> Vec<MoveDir> {
    let budget = scenario.budget.ticks as usize;
    let mut schedule = vec![MoveDir::NONE; budget];

    for span in &scenario.inputs {
        let dir = MoveDir::new(Vec3::new(span.dir.0, 0.0, span.dir.1));
        let from = span.at as usize;
        let to = from.saturating_add(span.ticks as usize).min(budget);

        for slot in schedule.iter_mut().take(to).skip(from) {
            *slot = dir;
        }
    }

    schedule
}

/// Checks the run against what the scenario predicted.
///
/// Collects every failure rather than stopping at the first. One run of the
/// simulation is microseconds, but a human or an agent reading the output pays
/// full price for each round trip — so a scenario that is wrong in three ways
/// should say so once.
pub(crate) fn check(scenario: &Scenario, run: &Run) -> Vec<Failure> {
    let mut failures = Vec::new();
    let Expect { player_pos, facing, contacts, enemy_count } = &scenario.expect;

    let ticks = run.world.tick();
    if ticks != scenario.budget.ticks {
        failures.push(Failure::new(
            "tick count",
            scenario.budget.ticks.to_string(),
            ticks.to_string(),
        ));
    }

    if let Some(want) = player_pos {
        let got = run.world.player_pos();
        let off = (glam::Vec2::new(got.x, got.z) - glam::Vec2::new(want.x, want.z)).length();

        if off > want.tol {
            let note = format!(
                "off by {off:.4}, tolerance {:.4} — that is {:.2} ticks of walking",
                want.tol,
                off / TICK_OF_WALKING,
            );

            failures.push(
                Failure::new(
                    "player_pos",
                    format!("({:.4}, {:.4}) +/- {:.4}", want.x, want.z, want.tol),
                    format!("({:.4}, {:.4})", got.x, got.z),
                )
                .with_note(note),
            );
        }
    }

    if let Some(want) = facing {
        let got = run.world.player_facing();
        if (got - want.value).abs() > want.tol {
            failures.push(
                Failure::new(
                    "facing",
                    format!("{:.4} +/- {:.4}", want.value, want.tol),
                    format!("{got:.4}"),
                )
                .with_note(format!("off by {:.4} radians", (got - want.value).abs())),
            );
        }
    }

    if let Some(want) = contacts {
        let got = run.world.contacts();
        if got != *want {
            failures.push(Failure::new("contacts", want.to_string(), got.to_string()));
        }
    }

    if let Some(want) = enemy_count {
        let got = run.world.enemy_count();
        if got != *want {
            failures.push(Failure::new("enemy_count", want.to_string(), got.to_string()));
        }
    }

    failures
}

/// Replays the scenario and compares hashes tick by tick.
///
/// **Run on every scenario, always.** It costs a second pass over microseconds
/// of work, and it means every scenario anyone writes for any reason is also a
/// determinism test — which is the property the entire verification story rests
/// on and the one nobody would remember to test on purpose.
pub(crate) fn check_replay(scenario: &Scenario, first: &Run) -> Option<Failure> {
    let second = run(scenario);

    let diverged = first
        .hashes
        .iter()
        .zip(&second.hashes)
        .position(|(a, b)| a != b)
        .or_else(|| (first.hashes.len() != second.hashes.len()).then_some(first.hashes.len()));

    let at = diverged?;

    Some(
        Failure::new("replay", "two runs identical every tick".into(), format!("diverged at tick {at}"))
            .with_note(
                "the simulation is not a pure function of (state, inputs). Look for a wall \
                 clock, a bare f32 where a Dt belongs, iteration over a hash-ordered \
                 container, unseeded randomness, or presentation state feeding back in"
                    .into(),
            ),
    )
}
