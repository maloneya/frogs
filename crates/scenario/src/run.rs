//! Running one scenario against the simulation.

use std::path::Path;

use arpg_core::MoveDir;
use arpg_sim::{Accumulator, Dt, EntityId, World};
use glam::Vec3;

use crate::spec::{Action, Expect, Scenario};

/// Everything one run produced. Kept separate from the checking so that the
/// determinism pass can compare two runs without re-deciding what "passed"
/// means.
pub(crate) struct Run {
    pub(crate) world: World,
    /// The world's hash after every tick, in order. This is what makes a
    /// divergence report a tick number instead of a shrug.
    pub(crate) hashes: Vec<u64>,
    /// The name each `Place` action got back, in the order they appear.
    ///
    /// The scenario file names bodies by placement number because it cannot
    /// hold an `EntityId`; this is the translation. Holding the real ids rather
    /// than dense indices is what makes an assertion survive a despawn moving
    /// rows around underneath it.
    ///
    /// `None` where a spawn was refused — the horde was already at the instance
    /// budget. Kept as a slot rather than dropped so that placement numbers do
    /// not shift under the assertions that refer to them.
    pub(crate) placed: Vec<Option<EntityId>>,
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

    // Setup actions come after the horde grid, so a scenario can put a body at
    // a known spot inside a crowd as well as in an empty arena.
    //
    // Applied strictly in order. A `Place` after a `Despawn` reuses the freed
    // slot, which is the only way a retired name can come back — so the order
    // here is what makes that case reachable at all.
    let mut placed: Vec<Option<EntityId>> = Vec::new();
    for action in &scenario.setup.actions {
        match action {
            // A refused spawn still takes a placement number, so every later
            // `nth` keeps pointing at the body its scenario meant. `None` is
            // what makes the next assertion about it fail and say so;
            // renumbering would instead re-point every later assertion at a
            // different body, silently.
            Action::Place((x, z)) => placed.push(world.spawn_enemy(glam::Vec2::new(*x, *z))),
            Action::Despawn(nth) => {
                if let Some(Some(id)) = placed.get(*nth).copied() {
                    world.despawn_enemy(id);
                }
            }
        }
    }

    // Setup is not the run. Without this the golden trace would also record
    // `World::default()` building a horde this scenario just replaced, tying
    // every golden file to a constant none of them are about.
    world.clear_trace();

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

    Run { world, hashes, placed }
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
    // Exhaustive, so an assertion added to the spec cannot be quietly left
    // unchecked — the same trick `World::hash` uses, for the same reason.
    let Expect { player_pos, facing, contacts, enemy_count, bodies, trace: _ } = &scenario.expect;

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

    for body in bodies {
        check_body(run, body, &mut failures);
    }

    failures
}

/// Checks one prediction about one placed body.
///
/// Looks the body up by the **name** it was given at spawn, never by a dense
/// index. That is the whole reason `EntityId` exists: the horde is stored
/// densely, so a despawn swaps the last row into the hole and every index after
/// it changes without anything touching those bodies.
fn check_body(run: &Run, body: &crate::spec::BodyExpect, failures: &mut Vec<Failure>) {
    let id = match run.placed.get(body.nth).copied() {
        Some(Some(id)) => id,
        Some(None) => {
            failures.push(
                Failure::new(
                    &format!("bodies[{}]", body.nth),
                    "a body".into(),
                    "the spawn was refused".into(),
                )
                .with_note("the horde was already at the instance budget".into()),
            );
            return;
        }
        None => {
            failures.push(
                Failure::new(
                    &format!("bodies[{}]", body.nth),
                    format!("a placement numbered {}", body.nth),
                    format!("only {} placement(s) were made", run.placed.len()),
                )
                .with_note("`nth` counts `Place` actions in `setup.actions`, from 0".into()),
            );
            return;
        }
    };

    let alive = run.world.is_alive(id);

    if let Some(want) = body.alive
        && alive != want
    {
        failures.push(
            Failure::new(
                &format!("bodies[{}].alive", body.nth),
                want.to_string(),
                alive.to_string(),
            )
            .with_note(if alive {
                format!("{id} still resolves — a despawned name must not come back")
            } else {
                format!("{id} is dead; it was despawned, or its slot was recycled")
            }),
        );
    }

    let Some(want) = &body.pos else { return };

    let Some(got) = run.world.enemy_pos(id) else {
        // Only reported when the scenario did not already say it expects this.
        // A file asserting `alive: false` and no position would otherwise fail
        // twice for one fact.
        if body.alive != Some(false) {
            failures.push(
                Failure::new(
                    &format!("bodies[{}].pos", body.nth),
                    format!("({:.4}, {:.4})", want.x, want.z),
                    "the body is dead".into(),
                )
                .with_note(format!("{id} no longer names a live body")),
            );
        }
        return;
    };

    let off = (glam::Vec2::new(got.x, got.z) - glam::Vec2::new(want.x, want.z)).length();
    if off > want.tol {
        failures.push(
            Failure::new(
                &format!("bodies[{}].pos", body.nth),
                format!("({:.4}, {:.4}) +/- {:.4}", want.x, want.z, want.tol),
                format!("({:.4}, {:.4})", got.x, got.z),
            )
            .with_note(format!("off by {off:.4}, tolerance {:.4}", want.tol)),
        );
    }
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

/// Compares the run's trace against a checked-in golden file.
///
/// Returns the failure, or `None` if it matched. `bless` rewrites the file
/// instead of comparing, which is the only way to create one.
pub(crate) fn check_trace(
    scenario: &Scenario,
    run: &Run,
    beside: &Path,
    bless: bool,
) -> Option<Failure> {
    let name = scenario.expect.trace.as_ref()?;
    let path = beside.parent().unwrap_or(Path::new(".")).join(name);
    let actual = run.world.trace().render();

    // A wrapped ring buffer means the beginning of the run is simply gone, so
    // the golden file would silently stop describing what it is named after.
    // Refused rather than compared.
    let dropped = run.world.trace().dropped();
    if dropped > 0 {
        return Some(
            Failure::new("trace", "a complete trace".into(), format!("{dropped} event(s) dropped"))
                .with_note(
                    "the trace ring buffer wrapped, so the start of the run is gone. Shorten \
                     the scenario or raise the buffer, but do not bless a truncated trace"
                        .into(),
                ),
        );
    }

    if bless {
        return match std::fs::write(&path, &actual) {
            Ok(()) => None,
            Err(e) => Some(Failure::new("trace", format!("write {}", path.display()), e.to_string())),
        };
    }

    let expected = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return Some(
                Failure::new("trace", format!("{}", path.display()), e.to_string()).with_note(
                    "no golden trace yet — create it with `--bless`, then read it before \
                     committing it"
                        .into(),
                ),
            );
        }
    };

    if expected == actual {
        return None;
    }

    // The first differing line, not a wall of text. A golden trace is hundreds
    // of lines and the useful information is almost always where they part.
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    let mut line = 0;

    loop {
        line += 1;
        match (expected_lines.next(), actual_lines.next()) {
            (Some(e), Some(a)) if e == a => continue,
            (e, a) => {
                return Some(
                    Failure::new(
                        "trace",
                        format!("{}:{line}: {}", path.display(), e.unwrap_or("<end of file>")),
                        format!("line {line}: {}", a.unwrap_or("<end of trace>")),
                    )
                    .with_note(format!(
                        "{} line(s) expected, {} produced — re-bless only once you \
                         understand why they differ",
                        expected.lines().count(),
                        actual.lines().count(),
                    )),
                );
            }
        }
    }
}
