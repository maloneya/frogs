//! Running one scenario against the simulation.

use std::path::Path;

use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::Game;
use arpg_sim::{Accumulator, Dt, EntityId, SourceId, Template};
use glam::Vec3;

use crate::spec::{Action, Expect, Scenario};

/// Everything one run produced. Kept separate from the checking so that the
/// determinism pass can compare two runs without re-deciding what "passed"
/// means.
pub(crate) struct Run {
    pub(crate) timing: crate::timing::Timing,
    pub(crate) game: Game,
    /// Stable identities of setup.sources, retained even after removal.
    sources: Vec<SourceId>,
    instances: Vec<Option<arpg_sim::SceneId>>,
    installed_sources: Vec<Vec<SourceId>>,
    /// Measured outside the simulation; never included in its replay state.
    mean_step_micros: f64,
    command_failures: Vec<Failure>,
    /// Evaluated against the live game at each checkpoint, never reconstructed
    /// from final state. Retain only failures, not copies of the game.
    checkpoint_failures: Vec<Failure>,
    /// The game's hash after every tick, in order. This is what makes a
    /// divergence report a tick number instead of a shrug.
    pub(crate) hashes: Vec<u64>,
    /// Stable IDs in placement order. Refused spawns retain `None` slots so later
    /// placement numbers do not shift.
    pub(crate) placed: Vec<Option<EntityId>>,
}

/// A single failed expectation, phrased so the message is the whole diagnosis.
#[derive(Clone)]
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

/// Fail closed on assertions that would otherwise never execute or be ignored.
/// The shared `Expect` keeps one definition of state assertions. Its trace field
/// is whole-run only; validating that restriction avoids duplicating the schema
/// just to remove one field from checkpoints.
pub(crate) fn validate(scenario: &Scenario) -> Vec<Failure> {
    let mut failures = Vec::new();
    // Every tick needs one entry in each schedule and the replay hash buffer.
    // Reject arithmetic/allocation-layout overflow before creating live state.
    let entry_bytes = size_of::<MoveDir>().max(size_of::<AimDir>()).max(size_of::<u64>());
    if usize::try_from(scenario.budget.ticks).ok()
        .and_then(|ticks| ticks.checked_mul(entry_bytes))
        .is_none_or(|bytes| bytes > isize::MAX as usize) {
        failures.push(Failure::new("budget", "a representable schedule length".into(), scenario.budget.ticks.to_string()));
    }
    if let Some(limit) = scenario.budget.max_mean_step_micros
        && (!limit.is_finite() || limit <= 0.0) {
        failures.push(Failure::new("budget.max_mean_step_micros", "a finite positive limit".into(), limit.to_string()));
    }
    for (name, at) in scenario.attacks.iter().map(|&at| ("attacks", at))
        .chain(scenario.interactions.iter().map(|&at| ("interactions", at)))
        .chain(scenario.spawns.iter().map(|command| ("spawns", command.at)))
        .chain(scenario.impulses.iter().map(|command| ("impulses", command.at)))
        .chain(scenario.tuning.iter().map(|command| ("tuning", command.at))) {
        if at >= scenario.budget.ticks {
            failures.push(Failure::new(name, "a tick inside the budget".into(), at.to_string()));
        }
    }
    for span in &scenario.inputs {
        validate_span("inputs", span.at, span.ticks, scenario.budget.ticks, &mut failures);
        let direction = Vec3::new(span.dir.0, 0.0, span.dir.1);
        if !direction.is_finite()
            || (direction != Vec3::ZERO && MoveDir::new(direction) == MoveDir::NONE) {
            failures.push(Failure::new("inputs", "zero or a finite normalizable movement direction".into(), format!("{:?}", span.dir)));
        }
    }
    if let Some(events) = &scenario.expect.events
        && (scenario.expect.trace.is_some() || events.kinds.is_empty() || events.sequence.len() >= arpg_sim::MAX_TRACE_EVENTS || events.sequence.iter().any(|(tick, event)|
            *tick >= scenario.budget.ticks || !events.kinds.contains(&event.kind()))) {
        failures.push(Failure::new("events", "events without a golden, a nonempty scope, and reachable predictions fitting MAX_TRACE_EVENTS".into(), format!("{events:?}")));
    }
    for aim in &scenario.aims {
        if aim.at >= scenario.budget.ticks || AimDir::new(Vec3::new(aim.dir.0, 0.0, aim.dir.1)).is_err() {
            failures.push(Failure::new("aims", "a finite nonzero aim and a tick inside the budget".into(), format!("{aim:?}")));
        }
    }
    let scenes: Vec<_> = scenario.setup.scenes.iter().chain(scenario.scene_commands().into_iter()
        .filter_map(|command| match &command.action {
            crate::spec::SceneAction::Load(scene) => Some(scene.as_ref()),
            crate::spec::SceneAction::Evict(_) => None,
        })).collect();
    let source_exists = |scene: Option<usize>, source: usize| match scene {
        None => source < scenario.setup.sources.len(),
        Some(nth) => scenes.get(nth).is_some_and(|scene| source < scene.engine.sources.len()),
    };
    for command in &scenario.source_switches {
        if command.at >= scenario.budget.ticks || !source_exists(command.scene, command.source) {
            failures.push(Failure::new(
                "source_switches",
                "an existing setup source and a tick inside the budget".into(),
                format!("source {} at tick {}", command.source, command.at),
            ));
        }
    }
    for expect in std::iter::once(&scenario.expect)
        .chain(scenario.checkpoints.iter().map(|checkpoint| &checkpoint.expect))
    {
        for torch in &expect.torches {
            if !scenes.get(torch.scene).is_some_and(|scene| torch.torch < scene.torches.len()) {
                failures.push(Failure::new("torches", "an authored scene torch".into(),
                    format!("scene {} torch {}", torch.scene, torch.torch)));
            }
        }
        for item in &expect.encounters {
            if !scenes.get(item.scene).is_some_and(|scene| item.encounter < scene.encounters.len()) {
                failures.push(Failure::new("encounters", "an authored scene encounter".into(), format!("scene {} encounter {}", item.scene, item.encounter)));
            }
        }
        for item in &expect.doors {
            if !scenes.get(item.scene).is_some_and(|scene| item.door < scene.doors.len()) {
                failures.push(Failure::new("doors", "an authored scene door".into(), format!("scene {} door {}", item.scene, item.door)));
            }
        }
        for item in &expect.chests {
            if !scenes.get(item.scene).is_some_and(|scene| item.chest < scene.chests.len()) {
                failures.push(Failure::new("chests", "an authored scene chest".into(), format!("scene {} chest {}", item.scene, item.chest)));
            }
        }
        for source in &expect.source_states {
            if !source_exists(source.scene, source.source) {
                failures.push(Failure::new(
                    "source_states", "an existing setup source index".into(),
                    source.source.to_string(),
                ));
            }
        }
    }
    for command in &scenario.remove_sources {
        if command.at >= scenario.budget.ticks || !source_exists(command.scene, command.source) {
            failures.push(Failure::new("remove_sources", "an authored source and reachable tick".into(),
                format!("source {} at tick {}", command.source, command.at)));
        }
    }
    for command in &scenario.despawns {
        if command.at >= scenario.budget.ticks {
            failures.push(Failure::new("despawns", "a reachable tick".into(), command.at.to_string()));
        }
    }
    for command in &scenario.scenes {
        if command.at >= scenario.budget.ticks {
            failures.push(Failure::new(
                "scenes",
                "a tick inside the budget".into(),
                command.at.to_string(),
            ));
        }
    }
    for hold in &scenario.channels {
        validate_span("channels", hold.at, hold.ticks, scenario.budget.ticks, &mut failures);
    }
    for command in &scenario.attack_recovery {
        if command.at >= scenario.budget.ticks {
            failures.push(Failure::new(
                "attack_recovery",
                format!("a tick below budget {}", scenario.budget.ticks),
                format!("tick {} will not run", command.at),
            ));
        }
    }
    for command in &scenario.attack_profiles {
        if command.at >= scenario.budget.ticks {
            failures.push(Failure::new(
                "attack_profiles",
                format!("a tick below budget {}", scenario.budget.ticks),
                format!("tick {} will not run", command.at),
            ));
        }
    }
    for (index, checkpoint) in scenario.checkpoints.iter().enumerate() {
        let label = format!("checkpoint[{}] after tick {}", index, checkpoint.at);
        if checkpoint.at >= scenario.budget.ticks {
            failures.push(Failure::new(
                &label,
                format!("a tick below budget {}", scenario.budget.ticks),
                format!("tick {} will not run", checkpoint.at),
            ));
        }
        if checkpoint.expect.trace.is_some() || checkpoint.expect.events.is_some() {
            failures.push(Failure::new(
                &label,
                "state assertions; transition evidence belongs in final expect.events or expect.trace".into(),
                "a checkpoint contains transition evidence".into(),
            ));
        }
    }
    failures
}

fn validate_span(name: &str, at: u64, ticks: u64, budget: u64, failures: &mut Vec<Failure>) {
    if ticks == 0 || at.checked_add(ticks).is_none_or(|end| end > budget) {
        failures.push(Failure::new(name, "a nonempty span inside the budget".into(), format!("{at} + {ticks}")));
    }
}

/// Runs the scenario to its budget.
///
/// Deliberately mints its `Dt` from a real [`Accumulator`], one tick's worth at
/// a time, rather than reaching for some test-only shortcut: the whole claim a
/// scenario makes is about the code path the game runs, and a runner that
/// stepped the game by some other route would be testing a different program.
pub(crate) fn run(scenario: &Scenario) -> Run {
    let run_started = std::time::Instant::now();
    let mut timing = crate::timing::Timing::default();
    let mut game = Game::empty();
    let mut instances = Vec::new();
    let mut installed_sources = Vec::new();
    let mut command_failures = Vec::new();

    // Setup actions follow authored scene construction.
    //
    // Applied strictly in order. A `Place` after a `Despawn` reuses the freed
    // slot, which is the only way a retired name can come back — so the order
    // here is what makes that case reachable at all.
    let mut placed: Vec<Option<EntityId>> = Vec::new();
    for scene in &scenario.setup.scenes {
        load_scene(&mut game, scene, &mut instances, &mut installed_sources, &mut placed, &mut command_failures);
    }
    for action in &scenario.setup.actions {
        match action {
            // A refused spawn still takes a placement number, so every later
            // `nth` keeps pointing at the body its scenario meant. `None` is
            // what makes the next assertion about it fail and say so;
            // renumbering would instead re-point every later assertion at a
            // different body, silently.
            Action::Place((x, z)) => {
                placed.push(game.place(glam::Vec2::new(*x, *z), Template::BODY));
            }
            // `spec::Action` is designed to grow, so resolving a placement
            // number is written once rather than pasted into each new arm.
            Action::Despawn(nth) => {
                if !placed.get(*nth).copied().flatten().is_some_and(|id| game.despawn_body(id)) {
                    command_failures.push(Failure::new("setup Despawn", "a live removable body".into(), format!("placement {nth}")));
                }
            }
            Action::Seek(nth) => {
                if !placed.get(*nth).copied().flatten().is_some_and(|id| game.add_seek(id)) {
                    command_failures.push(Failure::new("setup Seek", "a live body accepting seek".into(), format!("placement {nth}")));
                }
            }
        }
    }

    // Sources last, so a source is described against a game that is already
    // laid out — and, more practically, so that adding one cannot renumber the
    // placements a scenario's assertions refer to.
    // The name each source got, in the order the scenario listed them: the
    // translation from a scenario's source number, exactly as `placed` is for a
    // body's placement number.
    let sources: Vec<SourceId> =
        scenario.setup.sources.iter().map(|source| game.add_source(*source)).collect();

    // Setup is not the run. Golden traces start after authored scene loading
    // and initial placements so they describe the driven tick sequence.
    game.clear_trace();
    if scenario.expect.trace.is_some() {
        game.capture_trace(65_536).expect("fixed golden capacity is supported");
    }
    if let Some(events) = &scenario.expect.events {
        // One extra event proves an unexpected duplicate. Any larger excess
        // fails closed as overflow, without allocating according to crowd size.
        game.capture_events(events.sequence.len().saturating_add(1), events.kinds.clone()).expect("validated event count");
    }

    timing.setup = run_started.elapsed();
    let budget = scenario.budget.ticks as usize;
    let schedule = input_schedule(scenario);
    let aims = aim_schedule(scenario, &schedule);

    let mut accumulator = Accumulator::default();
    let mut hashes = Vec::with_capacity(budget);
    let mut step_time = std::time::Duration::ZERO;
    let mut checkpoint_failures = Vec::new();
    // Sort references once, retaining the file index for diagnostics. Duplicate
    // ticks remain distinct assertions, and work costs checkpoints + ticks.
    let mut checkpoints: Vec<_> = scenario.checkpoints.iter().enumerate().collect();
    checkpoints.sort_by_key(|(_, checkpoint)| checkpoint.at);
    let mut checkpoints = checkpoints.into_iter().peekable();
    let mut scene_commands = scenario.scene_commands().into_iter().peekable();

    while hashes.len() < budget {
        for dt in accumulator.pending(Dt::SECS) {
            let tick = hashes.len() as u64;
            // Loads complete before source evaluation; evictions therefore stop
            // a source from firing on this very tick. No implicit warm-up tick.
            while scene_commands.peek().is_some_and(|command| command.at == tick) {
                let command = scene_commands.next().expect("peeked scene command");
                match &command.action {
                    crate::spec::SceneAction::Load(scene) => {
                        load_scene(
                            &mut game,
                            scene,
                            &mut instances,
                            &mut installed_sources,
                            &mut placed,
                            &mut command_failures,
                        );
                    }
                    crate::spec::SceneAction::Evict(nth) => {
                        if !instances
                            .get(*nth)
                            .copied()
                            .flatten()
                            .is_some_and(|id| game.evict_scene(id))
                        {
                            command_failures.push(Failure::new(
                                "evict scene",
                                "a live instance".into(),
                                format!("instance {nth} absent at tick {tick}"),
                            ));
                        }
                    }
                }
            }
            for command in scenario.tuning.iter().filter(|command| command.at == tick) {
                game.set_tuning(command.values);
            }
            for command in scenario.attack_recovery.iter().filter(|command| command.at == tick) {
                game.set_attack_recovery(command.recovery);
            }
            for command in scenario.attack_profiles.iter().filter(|command| command.at == tick) {
                game.set_attack_profile(command.profile);
            }

            // Switches precede removals and source evaluation. Each uses the
            // same engine operation as the harness, with stale ids refused.
            for command in scenario.source_switches.iter().filter(|command| command.at == tick) {
                if !resolve_source(&sources, &installed_sources, command.scene, command.source).is_some_and(|id| {
                    game.set_source_enabled(id, command.enabled)
                }) {
                    command_failures.push(Failure::new(
                        "source_switches", "a live source".into(),
                        format!("source {} at tick {}", command.source, tick),
                    ));
                }
            }

            // Removals before the tick they name, so a source removed at `at`
            // does not fire on `at`. Stated in the spec, and it is the kind of
            // off-by-one that would otherwise be discovered by a golden trace
            // diff nobody could explain.
            for removal in scenario.remove_sources.iter().filter(|r| r.at == tick) {
                if !resolve_source(&sources, &installed_sources, removal.scene, removal.source)
                    .is_some_and(|id| game.remove_source(id)) {
                    command_failures.push(Failure::new("remove_sources", "a live source".into(),
                        format!("source {} at tick {tick}", removal.source)));
                }
            }

            for command in scenario.despawns.iter().filter(|command| command.at == tick) {
                if !placed.get(command.body).copied().flatten().is_some_and(|id| game.despawn_body(id)) {
                    command_failures.push(Failure::new("despawns", "a live removable body".into(),
                        format!("body {} at tick {tick}", command.body)));
                }
            }

            // Asked for before the step, through the same queue anything
            // inside the simulation uses. The spawn pass grants them before
            // row readers, so each body exists for the whole of tick `at`.
            let mut tickets = Vec::new();
            for spawn in scenario.spawns.iter().filter(|s| s.at == tick) {
                tickets.push(game.request_spawn(glam::Vec2::new(spawn.pos.0, spawn.pos.1), spawn.what));
            }

            for command in scenario.impulses.iter().filter(|command| command.at == tick) {
                let id = match command.target {
                    crate::spec::Target::Player => Some(game.player_id()),
                    crate::spec::Target::Placed(nth) => placed.get(nth).copied().flatten(),
                };
                if !id.is_some_and(|id| game.apply_impulse(id, command.value)) {
                    command_failures.push(Failure::new(
                        "impulse",
                        format!("a live target at tick {tick}"),
                        format!("{:?} is absent or dead", command.target),
                    ));
                }
            }
            let intent = Intent::new(schedule[tick as usize], aims[tick as usize], scenario.attacks.contains(&tick))
                .with_attack_held(scenario.channels.iter().any(|span| tick >= span.at && tick - span.at < span.ticks))
                .with_interact(scenario.interactions.contains(&tick));
            let started = std::time::Instant::now();
            game.step(dt, intent);
            step_time += started.elapsed();
            let hashing = std::time::Instant::now();
            hashes.push(game.hash());
            timing.hashing += hashing.elapsed();

            record_placed(&game, &tickets, &mut placed);
            // After recording this tick's spawns, so a checkpoint can inspect
            // a newly granted body by the same name used at the end of the run.
            let assertions = std::time::Instant::now();
            while checkpoints.peek().is_some_and(|(_, checkpoint)| checkpoint.at == tick) {
                let (index, checkpoint) = checkpoints.next().expect("peeked checkpoint");
                for mut failure in check_state(&checkpoint.expect, &game, &placed, &sources, &instances, &installed_sources) {
                    failure.what =
                        format!("checkpoint[{index}] after tick {tick}: {}", failure.what);
                    checkpoint_failures.push(failure);
                }
            }
            timing.assertions += assertions.elapsed();
        }
    }

    timing.simulation = step_time;
    timing.driver = run_started.elapsed().saturating_sub(
        timing.setup + timing.simulation + timing.hashing + timing.assertions);
    Run {
        timing,
        game,
        sources,
        instances,
        installed_sources,
        hashes,
        placed,
        mean_step_micros: step_time.as_secs_f64() * 1e6 / budget.max(1) as f64,
        command_failures,
        checkpoint_failures,
    }
}

/// Bind each explicit request by ticket, retaining refusals in their original
/// positions. Source requests follow explicit requests at the spawn boundary.
/// Scene installation binds its own immediate bodies at the load operation.
fn record_placed(
    game: &Game,
    tickets: &[Option<arpg_sim::SpawnTicket>],
    placed: &mut Vec<Option<EntityId>>,
) {
    let outcomes = game.spawn_outcomes();
    placed.extend(tickets.iter().map(|ticket| {
        ticket.map(|ticket| outcomes.iter().find(|result| result.ticket == ticket)
            .expect("each accepted spawn ticket completes at the boundary"))
            .and_then(|result| result.entity)
    }));
    placed.extend(outcomes.iter().filter(|result| result.source.is_some()).map(|result| result.entity));
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
        let to = from.checked_add(span.ticks as usize).expect("validated input span");

        for slot in &mut schedule[from..to] {
            *slot = dir;
        }
    }

    schedule
}

/// Device-free adapter. Every tick still supplies a concrete AimDir to Game.
fn aim_schedule(scenario: &Scenario, movement: &[MoveDir]) -> Vec<AimDir> {
    let mut aim = AimDir::FORWARD;
    movement.iter().enumerate().map(|(tick, dir)| {
        if scenario.aims.is_empty() {
            if let Ok(direction) = AimDir::new(dir.as_vec3()) { aim = direction; }
        } else {
            for command in scenario.aims.iter().filter(|a| a.at == tick as u64) {
                aim = AimDir::new(Vec3::new(command.dir.0, 0.0, command.dir.1))
                    .expect("scenario aim validated before running");
            }
        }
        aim
    }).collect()
}

/// Checks the run against what the scenario predicted.
///
/// Collects every failure rather than stopping at the first. One run of the
/// simulation is microseconds, but a human or an agent reading the output pays
/// full price for each round trip — so a scenario that is wrong in three ways
/// should say so once.
pub(crate) fn check(scenario: &Scenario, run: &Run) -> Vec<Failure> {
    let mut failures = Vec::new();
    failures.extend(run.command_failures.iter().cloned());
    failures.extend(run.checkpoint_failures.iter().cloned());
    if let Some(limit) = scenario.budget.max_mean_step_micros
        && (!limit.is_finite() || limit <= 0.0 || run.mean_step_micros > limit)
    {
        failures.push(Failure::new(
            "mean step time (microseconds)",
            format!("<= {limit}"),
            format!("{:.2}", run.mean_step_micros),
        ));
    }
    let ticks = run.game.tick();
    if ticks != scenario.budget.ticks {
        failures.push(Failure::new(
            "tick count",
            scenario.budget.ticks.to_string(),
            ticks.to_string(),
        ));
    }

    failures.extend(check_state(&scenario.expect, &run.game, &run.placed, &run.sources, &run.instances, &run.installed_sources));
    failures
}

/// One implementation for every observation point. Borrowing state prevents
/// checking a checkpoint from affecting the subsequent simulation or replay.
fn check_state(
    expect: &Expect, game: &Game, placed: &[Option<EntityId>], source_ids: &[SourceId],
    instances: &[Option<arpg_sim::SceneId>], installed_sources: &[Vec<SourceId>],
) -> Vec<Failure> {
    let mut failures = Vec::new();
    // Exhaustive, so an assertion added to the spec cannot be quietly left
    // unchecked — the same trick `Game::hash` uses, for the same reason.
    let Expect {
        player_health,
        attack_phase,
        swing_tick,
        recovery_ticks,
        swing_recovery_ticks,
        attack_profile,
        swing_profile,
        player_pos,
        player_velocity,
        facing,
        contacts,
        crowd_contacts,
        struck,
        hitbox,
        enemy_count,
        corpse_count,
        seekers,
        sources,
        source_states,
        encounters,
        doors,
        chests,
        scene_count,
        tree_count,
        torch_count,
        torches,
        bodies,
        trace: _,
        events: _,
    } = expect;

    check_eq("player_health", player_health, game.player_health(), &mut failures);
    let attack = game.attack_status();
    check_eq("attack_phase", attack_phase, attack.phase, &mut failures);
    check_eq("swing_tick", swing_tick, attack.elapsed, &mut failures);
    check_eq("recovery_ticks", recovery_ticks, attack.recovery.map_or(0, arpg_sim::RecoveryTicks::get), &mut failures);
    check_eq(
        "swing_recovery_ticks",
        swing_recovery_ticks,
        attack.swing_recovery.map_or(0, arpg_sim::RecoveryTicks::get),
        &mut failures,
    );
    check_eq("attack_profile", attack_profile, attack.profile, &mut failures);
    if let Some(want) = swing_profile
        && attack.swing_profile != Some(*want)
    {
        failures.push(Failure::new(
            "swing_profile",
            want.to_string(),
            attack.swing_profile.map_or("no swing in flight".into(), |got| got.to_string()),
        ));
    }

    failures.extend(check_finite(game));
    if let Some(want) = player_velocity {
        check_velocity("player_velocity", want, game.motion(game.player_id()), &mut failures);
    }
    if let Some(want) = player_pos {
        let got = game.player_pos();
        if let Some(off) = want.off_by(got) {
            failures.push(
                Failure::new(
                    "player_pos",
                    want.expected(),
                    format!("({:.4}, {:.4})", got.x, got.z),
                )
                .with_note(format!("position error {off:.4} metres; tolerance {:.4} metres", want.tol)),
            );
        }
    }

    if let Some(want) = facing {
        let got = game.player_facing();
        if !want.matches(got) {
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

    check_eq("contacts", contacts, game.contacts(), &mut failures);
    check_eq("crowd_contacts", crowd_contacts, game.crowd_contacts(), &mut failures);
    check_eq("struck", struck, game.struck(), &mut failures);
    check_eq("hitbox", hitbox, game.hitbox_is_live(), &mut failures);
    check_eq("corpse_count", corpse_count, game.corpses().len(), &mut failures);
    check_eq("enemy_count", enemy_count, game.enemy_count(), &mut failures);
    check_eq("seekers", seekers, game.seeker_count(), &mut failures);
    check_eq("sources", sources, game.source_count(), &mut failures);
    for expected in source_states {
        let actual = resolve_source(source_ids, installed_sources, expected.scene, expected.source).and_then(|id| game.source_state(id));
        if actual != expected.state {
            failures.push(Failure::new(
                &format!("source_states[{}]", expected.source),
                format!("{:?}", expected.state), format!("{actual:?}"),
            ));
        }
    }
    for expected in encounters {
        let actual = instances.get(expected.scene).copied().flatten().and_then(|id| game.encounter_state(id, expected.encounter));
        if actual != expected.state {
            failures.push(Failure::new(&format!("encounters[{}/{}]", expected.scene, expected.encounter), format!("{:?}", expected.state), format!("{actual:?}")));
        }
    }
    for expected in doors {
        let actual = instances.get(expected.scene).copied().flatten().and_then(|id| game.door_state(id, expected.door));
        if actual != expected.state {
            failures.push(Failure::new(&format!("doors[{}/{}]", expected.scene, expected.door), format!("{:?}", expected.state), format!("{actual:?}")));
        }
    }
    for expected in chests {
        let actual = instances.get(expected.scene).copied().flatten().and_then(|id| game.chest_state(id, expected.chest));
        if actual != expected.state {
            failures.push(Failure::new(&format!("chests[{}/{}]", expected.scene, expected.chest), format!("{:?}", expected.state), format!("{actual:?}")));
        }
    }
    check_eq("tree_count", tree_count, game.trees().count(), &mut failures);
    check_eq("torch_count", torch_count, game.torch_count(), &mut failures);
    for expected in torches {
        let actual = instances.get(expected.scene).copied().flatten()
            .and_then(|id| game.scene_torch(id, expected.torch));
        if actual != expected.state {
            failures.push(Failure::new(&format!("torches[{}/{}]", expected.scene, expected.torch),
                format!("{:?}", expected.state), format!("{actual:?}")));
        }
    }
    check_eq("scene_count", scene_count, game.scene_count(), &mut failures);

    for body in bodies {
        check_body(game, placed, body, &mut failures);
    }

    failures
}

/// Compares one predicted value, when the scenario predicted one.
///
/// The label and the accessor were paired by hand at each of five call sites,
/// and nothing checked that they matched — so a block pasted from its neighbour
/// with the accessor updated and the label forgotten would assert the right
/// value under the wrong name, and point the reader at a field that is fine.
fn check_eq<T: PartialEq + std::fmt::Display>(
    what: &str,
    want: &Option<T>,
    got: T,
    failures: &mut Vec<Failure>,
) {
    if let Some(want) = want
        && got != *want
    {
        failures.push(Failure::new(what, want.to_string(), got.to_string()));
    }
}

/// Checks one prediction about one placed body.
///
/// Looks the body up by the **name** it was given at spawn, never by a dense
/// index. That is the whole reason `EntityId` exists: the horde is stored
/// densely, so a despawn swaps the last row into the hole and every index after
/// it changes without anything touching those bodies.
fn check_body(
    game: &Game,
    placed: &[Option<EntityId>],
    body: &crate::spec::BodyExpect,
    failures: &mut Vec<Failure>,
) {
    let id = match placed.get(body.nth).copied() {
        Some(Some(id)) => id,
        Some(None) => {
            failures.push(
                Failure::new(
                    &format!("bodies[{}]", body.nth),
                    "a body".into(),
                    "the spawn was refused".into(),
                )
                .with_note("capacity was exhausted or the position/template was invalid".into()),
            );
            return;
        }
        None => {
            failures.push(
                Failure::new(
                    &format!("bodies[{}]", body.nth),
                    format!("a placement numbered {}", body.nth),
                    format!("only {} placement(s) were made", placed.len()),
                )
                .with_note("`nth` counts `Place` actions in `setup.actions`, from 0".into()),
            );
            return;
        }
    };

    if let Some(want) = body.interaction {
        let got = game.interaction_state(id);
        if got != Some(want) {
            failures.push(Failure::new(&format!("bodies[{}].interaction", body.nth),
                want.to_string(), got.map_or("no interaction".into(), |state| state.to_string())));
        }
    }
    if let Some(want) = body.gait {
        let got = game.gait(id);
        if got != Some(want) {
            failures.push(Failure::new(&format!("bodies[{}].gait", body.nth),
                want.label().into(), got.map_or("no gait", arpg_sim::Gait::label).into()));
        }
    }
    let alive = game.is_alive(id);
    check_eq(&format!("bodies[{}].damageable", body.nth), &body.damageable,
        game.health(id).is_some(), failures);

    check_eq(
        &format!("bodies[{}].seeking", body.nth),
        &body.seeking,
        game.is_seeker(id),
        failures,
    );

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

    if let Some(want) = &body.velocity {
        check_velocity(&format!("bodies[{}].velocity", body.nth), want, game.motion(id), failures);
    }
    if let Some(want) = body.health {
        let got = game.health(id);
        if got != Some(want) {
            failures.push(Failure::new(
                &format!("bodies[{}].health", body.nth),
                want.to_string(),
                got.map_or("no live enemy health".into(), |value| value.to_string()),
            ));
        }
    }
    let Some(want) = &body.pos else { return };

    let Some(got) = game.body_pos(id) else {
        // A requested position must be observed even when alive:false also
        // appears: asserting absence cannot disable another assertion.
        failures.push(
            Failure::new(
                &format!("bodies[{}].pos", body.nth),
                want.expected(),
                "the body is dead".into(),
            )
            .with_note(format!("{id} no longer names a live body")),
        );
        return;
    };

    if let Some(off) = want.off_by(got) {
        failures.push(
            Failure::new(
                &format!("bodies[{}].pos", body.nth),
                want.expected(),
                format!("({:.4}, {:.4})", got.x, got.z),
            )
            .with_note(format!("off by {off:.4}, tolerance {:.4}", want.tol)),
        );
    }
}

/// Rejects non-finite positions, including bodies without explicit predictions.
/// This cannot detect NaNs already hidden by clamping; containment checks earlier.
fn check_finite(game: &Game) -> Option<Failure> {
    if game.all_positions_finite() {
        return None;
    }

    Some(
        Failure::new(
            "finite",
            "every position a real number".into(),
            "a position is NaN or infinite".into(),
        )
        .with_note(
            "the solver poisoned a position, and no clamp recovers one. Look for a \
                 normalise of a zero-length difference — two bodies at exactly the same \
                 point — or a divide by a combined mass of zero"
                .into(),
        ),
    )
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
        Failure::new(
            "replay",
            "two runs identical every tick".into(),
            format!("diverged at tick {at}"),
        )
        .with_note(
            "the simulation is not a pure function of (state, inputs). Look for a wall \
                 clock, a bare f32 where a Dt belongs, iteration over a hash-ordered \
                 container, unseeded randomness, or presentation state feeding back in"
                .into(),
        ),
    )
}

/// Check exact typed event predictions or a diagnostic serialization golden.
/// `bless` can rewrite only a complete golden; typed predictions always assert.
pub(crate) fn check_trace(
    scenario: &Scenario,
    run: &Run,
    beside: &Path,
    bless: bool,
) -> Option<Failure> {
    if let Some(events) = &scenario.expect.events {
        let Some(actual) = run.game.captured_events() else {
            return Some(Failure::new("events", "complete selected evidence".into(), "event capture overflowed".into()));
        };
        if actual.len() != events.sequence.len() {
            return Some(Failure::new("events", format!("{} selected events", events.sequence.len()), format!("{} selected events", actual.len())));
        }
        for (index, ((tick, expected), (actual_tick, event))) in events.sequence.iter().zip(actual).enumerate() {
            if tick != actual_tick || !expected.matches(*event) {
                return Some(Failure::new("events", format!("event {index}: {tick} {expected:?}"), format!("{actual_tick} {event:?}")));
            }
        }
    }
    let name = scenario.expect.trace.as_ref()?;
    let path = beside.parent().unwrap_or(Path::new(".")).join(name);
    let Some(actual) = run.game.render_captured_trace() else {
        return Some(Failure::new("trace", "complete captured evidence".into(), "capture overflowed".into()));
    };

    if bless {
        return match std::fs::write(&path, &actual) {
            Ok(()) => None,
            Err(e) => {
                Some(Failure::new("trace", format!("write {}", path.display()), e.to_string()))
            }
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

fn check_velocity(
    name: &str,
    want: &crate::spec::Approx2,
    motion: Option<arpg_sim::Motion>,
    failures: &mut Vec<Failure>,
) {
    let Some(motion) = motion else {
        failures.push(Failure::new(name, want.expected(), "no physical body".into()));
        return;
    };
    let v = motion.velocity();
    if want.off_by(Vec3::new(v.x, 0.0, v.y)).is_some() {
        failures.push(Failure::new(name, want.expected(), format!("({:.6}, {:.6})", v.x, v.y)));
    }
}

fn load_scene(
    game: &mut Game,
    scene: &arpg_game::GameScene,
    instances: &mut Vec<Option<arpg_sim::SceneId>>,
    installed_sources: &mut Vec<Vec<SourceId>>,
    placed: &mut Vec<Option<EntityId>>,
    failures: &mut Vec<Failure>,
) {
    match game.load_scene(scene) {
        Ok(id) => {
            instances.push(Some(id));
            installed_sources.push(game.scene_sources(id).unwrap_or_default().to_vec());
            placed.extend(game.scene_bodies(id).unwrap_or_default().iter().copied().map(Some));
        }
        Err(error) => {
            instances.push(None);
            installed_sources.push(Vec::new());
            failures.push(Failure::new("load scene", "a ready scene".into(), error.to_string()));
        }
    }
}

fn resolve_source(setup: &[SourceId], installed: &[Vec<SourceId>],
    scene: Option<usize>, source: usize) -> Option<SourceId> {
    match scene {
        None => setup.get(source),
        Some(nth) => installed.get(nth).and_then(|sources| sources.get(source)),
    }.copied()
}

#[cfg(test)]
mod channel_input_tests {
    use super::*;

    #[test]
    fn channel_spans_reject_empty_unreachable_and_overflowing_input() {
        for span in ["(at: 0, ticks: 0)", "(at: 10, ticks: 1)", "(at: 9, ticks: 2)", "(at: 1, ticks: 18446744073709551615)"] {
            let source = format!("(description: \"refused channel\", channels: [{span}], budget: (ticks: 10))");
            let scenario: Scenario = ron::from_str(&source).unwrap();
            assert!(validate(&scenario).iter().any(|failure| failure.what == "channels"));
        }
        let scenario: Scenario = ron::from_str("(description: \"valid channel\", channels: [(at: 0, ticks: 10)], budget: (ticks: 10))").unwrap();
        assert!(validate(&scenario).is_empty());
    }
}

#[cfg(test)]
mod spawn_binding_tests {
    use super::*;

    #[test]
    fn refusal_keeps_its_place_and_log_retention_cannot_change_bindings() {
        for retention in [0, 1, 16_384] {
            let mut game = Game::empty();
            game.set_trace_retention(retention).unwrap();
            let tickets = [
                game.request_spawn(glam::Vec2::new(10.0, 0.0), Template::BODY),
                game.request_spawn(glam::Vec2::splat(f32::NAN), Template::BODY),
                // An immediately refused request must also retain its number.
                None,
                game.request_spawn(glam::Vec2::new(20.0, 0.0), Template::BODY),
            ];
            for dt in Accumulator::default().pending(Dt::SECS) {
                game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
            }
            game.clear_trace();

            let mut placed = Vec::new();
            record_placed(&game, &tickets, &mut placed);
            assert_eq!(placed.len(), 4);
            assert!(placed[0].is_some());
            assert_eq!(placed[1], None);
            assert_eq!(placed[2], None);
            assert!(placed[3].is_some());
            assert_ne!(placed[0], placed[3]);
        }
    }
}
