//! Melee timing and hit resolution from committed swing samples.
//!
//! Hits submit impulses; displacement begins at the next motion pass.

use crate::{AttackProfile, AttackStatus, GameplayEvent as Event, Tuning, TuningField};
use arpg_core::Intent;
use arpg_sim::{
    ActionControls, ActionExecution, ActionSchedule, AttackDisc, AttackPhase, Beam, Dt, EntityId,
    Fnv, Hitbox, Impulse, Interruption, MotorCommand, ResolvedAttack, ResolvedStep, TraceSink,
};
use glam::{Vec2, Vec3Swizzles};

/// Authored Disintegrate policy. The simulation only receives a capsule and effects.
pub struct Disintegrate;
impl Disintegrate {
    /// Reach from the actor's centre, in metres.
    pub const LENGTH: f32 = 12.0;
    /// Half width, in metres.
    pub const RADIUS: f32 = 1.2;
    /// Momentum along the beam per touching tick, in kg m/s.
    pub const KNOCKBACK: f32 = 0.15;
    /// One damage point immediately, then once every twelve ticks.
    pub const HIT_EVERY: u32 = 12;
}
fn beam(origin: Vec2, facing: f32) -> Beam {
    Beam::new(
        origin,
        origin + Vec2::new(facing.sin(), facing.cos()) * Disintegrate::LENGTH,
        Disintegrate::RADIUS,
    )
    .expect("finite player pose and authored beam")
}

/// Maximum stored discs: one per tick of the longest accepted active window.
const HITBOX_SAMPLES: usize = ResolvedAttack::MAX_ACTIVE_TICKS as usize;

/// A swing in the air: how far through it is, and the shape it committed to.
///
/// **The two are one value, so a hitbox cannot exist without a timer to say
/// which part of it is live.** Kept as separate fields on [`Attack`] they were
/// two things that had to agree, and nothing but convention said a caller had
/// to check the timer before reading the shape.
struct InFlight {
    /// Ticks since the swing began. Names the tick that just ran — see
    /// [`attack`] for why it is advanced before anything reads it.
    execution: ActionExecution,
    /// Authored move identity captured at the same boundary as its resolved values.
    profile: AttackProfile,
    /// Captured at the press; later resolution cannot reshape this swing.
    resolved: ResolvedAttack,
    /// Committed local hitbox samples; local coordinates let the swing follow player motion.
    hitbox: Hitbox<HITBOX_SAMPLES>,
}

impl InFlight {
    /// The shared lifecycle supplies the active sample index and phase edges.
    fn phase(&self) -> Phase {
        match self.execution.phase() {
            (AttackPhase::Startup, _, _) => Phase::Startup,
            (AttackPhase::Active, sample, _) => Phase::Active(sample as usize),
            (AttackPhase::Recovery, _, _) => Phase::Recovery,
            (AttackPhase::Idle, _, _) => {
                unreachable!("retired swings are removed before observation")
            }
        }
    }
}

/// Where the swing has got to.
///
/// **One `Option`, with every phase derived from it**, rather than a stored
/// phase beside a counter. Two fields that must agree are two fields that can
/// disagree, and the disagreement here would be a hitbox live during recovery —
/// silent, and visible only as an attack that feels wrong.
pub(crate) struct Attack {
    /// The swing in the air, or `None` when idle.
    swing: Option<InFlight>,
    /// Authored move selected for the next swing.
    profile: AttackProfile,
    /// The next swing's complete configuration; the current swing owns a copy.
    resolved: Option<ResolvedAttack>,
    /// Tick within the beam damage cadence; no body identities are retained.
    channel: Option<Channel>,
    /// Bodies already struck, preventing repeated physical impacts across active ticks.
    struck: Vec<EntityId>,
    /// Damage positions from this completed tick, including defeated targets.
    beam_hits: Vec<Vec2>,
    /// Reused query results bridge immutable physical observation and explicit effects.
    /// Cleared before and after resolution; never authoritative state.
    contacts: Vec<(EntityId, Vec2, Vec2)>,
}

struct Channel {
    execution: ActionExecution,
    /// Keep pulse cadence independent of the saturating lifetime clock.
    cadence: u32,
}
impl Channel {
    fn new() -> Self {
        Self {
            execution: ActionExecution::new(
                ActionSchedule::Open,
                ActionControls::Movement,
                Interruption::Interruptible,
            ),
            cadence: 0,
        }
    }
    fn advance(&mut self) {
        self.execution.advance();
        self.cadence = (self.cadence + 1) % Disintegrate::HIT_EVERY;
    }
}

impl Default for Attack {
    fn default() -> Self {
        Self {
            swing: None,
            channel: None,
            profile: AttackProfile::default(),
            resolved: AttackProfile::default().resolve(Tuning::default()),
            // Match physical admission, as defeat storage does. Neither the
            // first tick nor a wide hit against an existing crowd may allocate.
            struck: Vec::with_capacity(arpg_sim::MAX_BODIES),
            beam_hits: Vec::with_capacity(arpg_sim::MAX_BODIES),
            contacts: Vec::with_capacity(arpg_sim::MAX_BODIES),
        }
    }
}

impl Attack {
    /// Channel admission for the upcoming tick, shared by walking, aim and damage.
    pub(crate) fn channels(&self, held: bool) -> bool {
        held && self.profile == AttackProfile::Disintegrate && !self.holds_facing()
    }

    pub(crate) fn beam(&self, origin: Vec2, facing: f32) -> Option<Beam> {
        self.channel.as_ref().map(|_| beam(origin, facing))
    }

    pub(crate) fn beam_hits(&self) -> &[Vec2] {
        &self.beam_hits
    }

    /// Whether the current swing still owns facing on the upcoming tick.
    /// Retirement happens before new input, matching the attack pass below.
    pub(crate) fn holds_facing(&self) -> bool {
        self.swing
            .as_ref()
            .is_some_and(|swing| swing.execution.holds_next_tick(ActionControls::Facing))
    }

    /// A fresh observation, never a second maintained copy of attack state.
    pub(crate) fn status(&self) -> AttackStatus {
        let Self {
            swing,
            profile,
            resolved,
            channel,
            struck,
            beam_hits: _,
            contacts: _,
        } = self;
        let phase = match swing.as_ref().map(InFlight::phase) {
            None if channel.is_some() => AttackPhase::Active,
            None => AttackPhase::Idle,
            Some(Phase::Startup) => AttackPhase::Startup,
            Some(Phase::Active(_)) => AttackPhase::Active,
            Some(Phase::Recovery) => AttackPhase::Recovery,
        };
        AttackStatus {
            execution: swing
                .as_ref()
                .map(|s| s.execution)
                .or_else(|| channel.as_ref().map(|c| c.execution)),
            phase,
            channeling: channel.is_some(),
            channel_tick: channel.as_ref().map(|channel| channel.cadence),
            elapsed: swing.as_ref().map_or(0, |s| s.execution.elapsed()),
            profile: *profile,
            swing_profile: swing.as_ref().map(|s| s.profile),
            resolved: *resolved,
            swing_resolved: swing.as_ref().map(|s| s.resolved),
            struck: struck.len(),
        }
    }

    /// Observe the stored path during startup, or the exact live sample during active.
    pub(crate) fn debug_discs(
        &self,
        origin: Vec2,
        facing: f32,
    ) -> impl Iterator<Item = AttackDisc> + '_ {
        let (discs, first) = match self.swing.as_ref() {
            Some(swing) => match swing.phase() {
                Phase::Startup => (swing.hitbox.discs(), 0),
                Phase::Active(sample) => (&swing.hitbox.discs()[sample..=sample], sample),
                Phase::Recovery => (&[][..], 0),
            },
            None => (&[][..], 0),
        };
        discs.iter().enumerate().map(move |(offset, disc)| {
            let (centre, radius) = disc.place(origin, facing);
            AttackDisc::new(
                glam::Vec3::new(centre.x, 0.0, centre.y),
                radius,
                first + offset,
            )
        })
    }

    pub(crate) fn set_profile(&mut self, profile: AttackProfile, tuning: Tuning) -> bool {
        let resolved = profile.resolve(tuning);
        let changed = self.profile != profile || self.resolved != resolved;
        self.profile = profile;
        self.resolved = resolved;
        changed
    }

    pub(crate) fn retune(&mut self, tuning: Tuning) {
        self.resolved = self.profile.resolve(tuning);
    }

    /// Whether the hitbox exists right now.
    pub(crate) fn hitbox_is_live(&self) -> bool {
        self.channel.is_some()
            || matches!(
                self.swing.as_ref().map(InFlight::phase),
                Some(Phase::Active(_))
            )
    }

    /// How many bodies the current or most recent swing struck.
    pub(crate) fn struck(&self) -> usize {
        self.struck.len()
    }

    /// Feeds the swing into the world hash. Exhaustive, as every hash here is.
    pub(crate) fn hash(&self, h: &mut Fnv) {
        let Self {
            swing,
            profile,
            resolved,
            channel,
            struck,
            beam_hits,
            contacts: _,
        } = self;
        h.usize(*profile as usize);
        if let Some(resolved) = resolved {
            resolved.hash_into(h);
        }
        h.usize(usize::from(channel.is_some()));
        if let Some(Channel { execution, cadence }) = channel {
            execution.hash_into(h);
            h.u64(u64::from(*cadence));
        }

        // `None` and "tick 0 of a swing" are different states and must hash
        // differently, so the discriminant goes in as well as the value.
        h.usize(usize::from(swing.is_some()));
        if let Some(InFlight {
            execution,
            profile,
            resolved,
            hitbox,
        }) = swing
        {
            execution.hash_into(h);
            h.usize(*profile as usize);
            resolved.hash_into(h);
            hitbox.hash_into(h);
        }

        h.usize(beam_hits.len());
        for point in beam_hits {
            h.f32(point.x);
            h.f32(point.y);
        }
        h.usize(struck.len());
        for id in struck {
            id.hash_into(h);
        }
    }
}

/// Which part of the swing a tick falls in, and what that part has.
///
/// The three configured phases, as a thing a caller can match on
/// rather than three comparisons it has to get the boundaries right on.
enum Phase {
    /// Committed wind-up, with no live hitbox yet.
    Startup,
    /// The hitbox exists, and this is which of its discs is live.
    Active(usize),
    /// The hitbox is gone, and no new swing may start yet.
    Recovery,
}

/// Resolve authored player effects after physics, before autonomous effects and retirement.
pub(crate) fn attack(
    state: &mut Attack,
    facing: f32,
    input: Intent,
    eligible: impl Fn(EntityId) -> bool,
    resolved_step: &mut ResolvedStep<'_>,
    mut trace: TraceSink<'_, Event>,
) {
    let actor = resolved_step.view().player_id();
    let origin = resolved_step
        .view()
        .position(actor)
        .expect("persistent player body");
    state.beam_hits.clear();
    let channel_requested = state.channels(input.attack_held());
    // Advance before accepting input so post-step observations describe the tick
    // just processed, and the first tick after recovery can accept a new swing.
    if let Some(swing) = &mut state.swing {
        swing.execution.advance();
    }
    if state
        .swing
        .as_ref()
        .is_some_and(|swing| !swing.execution.is_running())
    {
        state.swing = None;
    }

    if !channel_requested && let Some(mut channel) = state.channel.take() {
        channel.execution.cancel();
        trace.emit(Event::ChannelStopped);
    }
    if channel_requested {
        let cadence = if let Some(channel) = &mut state.channel {
            channel.advance();
            channel.cadence
        } else {
            state.channel = Some(Channel::new());
            trace.emit(Event::ChannelStarted);
            0
        };
        state.struck.clear();
        let beam = resolved_step.view().clip_beam(beam(origin, facing));
        let push = (beam.end() - beam.start()).normalize_or_zero() * Disintegrate::KNOCKBACK;
        let impulse = Impulse::try_from((push.x, push.y)).expect("finite authored beam impulse");
        state.contacts.clear();
        for (target, point) in resolved_step.view().beam_contacts(beam) {
            if target != actor {
                state.contacts.push((
                    target,
                    point,
                    resolved_step
                        .view()
                        .position(target)
                        .expect("contact is live"),
                ));
            }
        }
        for &(target, point, position) in &state.contacts {
            resolved_step.impact(actor, target, point, impulse);
            if cadence == 0
                && eligible(target)
                && resolved_step
                    .damage(actor, target, core::num::NonZeroU8::MIN)
                    .is_some()
            {
                state.beam_hits.push(position);
            }
        }
        state.contacts.clear();
        return;
    }
    if state.swing.is_none()
        && input.attack()
        && let Some(resolved) = state.resolved
    {
        let profile = state.profile;
        state.swing = Some(InFlight {
            execution: ActionExecution::new(
                ActionSchedule::timed(
                    resolved.startup(),
                    resolved.active(),
                    resolved.recovery().get(),
                )
                .expect("resolved attack has validated timing"),
                ActionControls::Facing,
                Interruption::Committed,
            ),
            profile,
            resolved,
            hitbox: resolved
                .hitbox()
                .expect("validated active window fits committed storage"),
        });
        state.struck.clear();
        trace.emit(Event::Swung);
    }

    let Some(swing) = &state.swing else { return };
    let elapsed = swing.execution.elapsed();

    // Taken while the swing is only borrowed, so `strike` can have the `struck`
    // list mutably. `Disc` is `Copy` and twelve bytes, so this is a register
    // move rather than a compromise.
    let live = match swing.phase() {
        Phase::Active(sample) => Some(swing.hitbox.at(sample)),
        Phase::Startup | Phase::Recovery => None,
    };

    let resolved = swing.resolved;
    if elapsed == resolved.startup() {
        trace.emit(Event::HitboxOpened);
    }

    if let Some(disc) = live {
        let (centre, radius) = disc.place(origin, facing);
        state.contacts.clear();
        for (target, point) in resolved_step.view().disc_contacts(origin, centre, radius) {
            if target != actor && !state.struck.contains(&target) {
                state.contacts.push((
                    target,
                    point,
                    resolved_step
                        .view()
                        .position(target)
                        .expect("contact is live"),
                ));
            }
        }
        for &(target, point, position) in &state.contacts {
            state.struck.push(target);
            if eligible(target) {
                resolved_step.damage(actor, target, resolved.damage());
            }
            let direction = if swing.profile == AttackProfile::Slam {
                (position - origin).normalize_or(Vec2::X)
            } else {
                Vec2::new(facing.sin(), facing.cos())
            } * resolved.knockback();
            resolved_step.impact(
                actor,
                target,
                point,
                Impulse::try_from((direction.x, direction.y))
                    .expect("finite authored melee impulse"),
            );
        }
        state.contacts.clear();
    }

    // Symmetric with the open above: `opened` on the first tick the hitbox
    // exists, `closed` on the first tick it does not. Emitting `closed` on the
    // *last* live tick instead would read as a one-tick-shorter window in every
    // trace, which is precisely the confusion these events exist to prevent.
    // Both land inside the swing because recovery is at least one tick.
    if elapsed == resolved.startup() + resolved.active() {
        trace.emit(Event::HitboxClosed);
    }
}

/// Player-owned ability execution and facing. Physics receives only a motor command.
#[derive(Default)]
pub(crate) struct Player {
    pub(crate) attack: Attack,
    facing: f32,
    prev_facing: f32,
}
impl Player {
    pub(crate) fn prepare(
        &mut self,
        actor: EntityId,
        input: Intent,
        dt: Dt,
        tuning: Tuning,
    ) -> Option<MotorCommand> {
        self.prev_facing = self.facing;
        let channel = self.attack.channels(input.attack_held());
        if !self.attack.holds_facing() {
            let aim = input.aim().as_vec3();
            let target = aim.x.atan2(aim.z);
            if input.attack() || channel {
                self.facing = target;
            } else {
                let arc = arpg_sim::angle::shortest_arc(self.facing, target);
                let step = (14.0 * dt.secs()).min(arc.abs());
                self.facing = arpg_sim::angle::wrap(self.facing + step * arc.signum());
            }
        }
        (!channel).then(|| {
            MotorCommand::new(
                actor,
                input.move_dir().as_vec3().xz() * tuning.get(TuningField::PlayerSpeed),
                tuning.get(TuningField::PlayerAcceleration),
            )
            .expect("validated player motor tuning")
        })
    }
    pub(crate) fn facing(&self) -> f32 {
        self.facing
    }
    pub(crate) fn drawn_facing(&self, alpha: arpg_sim::Alpha) -> f32 {
        let arc = arpg_sim::angle::shortest_arc(self.prev_facing, self.facing);
        arpg_sim::angle::wrap(self.prev_facing + arc * alpha.get())
    }
    pub(crate) fn hash(&self, hash: &mut Fnv) {
        let Self {
            attack,
            facing,
            prev_facing,
        } = self;
        attack.hash(hash);
        hash.f32(*facing);
        hash.f32(*prev_facing);
    }
    pub(crate) fn report(&self, view: arpg_sim::ActorView<'_>, out: &mut arpg_core::Report) {
        let Self {
            attack,
            facing,
            prev_facing,
        } = self;
        attack.status().report(out);
        out.num("facing", *facing);
        out.num("prev_facing", *prev_facing);
        out.object("beam", |out| {
            if let Some(beam) = attack.beam(
                view.position(view.player_id())
                    .expect("persistent player body"),
                *facing,
            ) {
                let beam = view.clip_beam(beam);
                let on_ground = |point: Vec2| glam::Vec3::new(point.x, 0.0, point.y);
                out.vec3("start", on_ground(beam.start()));
                out.vec3("end", on_ground(beam.end()));
                out.num("radius", beam.radius());
                out.num("knockback_per_tick", Disintegrate::KNOCKBACK);
                out.int("hits", attack.beam_hits().len() as u64);
                out.object("hit_positions", |out| {
                    for (index, point) in attack.beam_hits().iter().enumerate() {
                        out.vec3(&index.to_string(), on_ground(*point));
                    }
                });
                out.int("hit_every_ticks", u64::from(Disintegrate::HIT_EVERY));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channel_presence_and_damage_clock_reach_hash_and_report() {
        let mut state = Attack::default();
        state.set_profile(AttackProfile::Disintegrate, crate::Tuning::default());
        let hash = |state: &Attack| {
            let mut hash = Fnv::default();
            state.hash(&mut hash);
            hash.finish()
        };
        let idle = hash(&state);
        state.channel = Some(Channel::new());
        let first = hash(&state);
        assert_ne!(idle, first);
        state.channel.as_mut().unwrap().advance();
        assert_ne!(first, hash(&state));
        let before_hits = hash(&state);
        state.beam_hits.push(Vec2::new(0.0, 3.0));
        assert_ne!(
            before_hits,
            hash(&state),
            "completed damage positions enter the hash"
        );
        let status = state.status();
        assert!(status.channeling);
        assert_eq!(status.channel_tick, Some(1));
        let execution = status.execution.unwrap();
        assert_eq!(execution.elapsed(), 1);
        assert!(execution.holds(ActionControls::Movement));
        assert!(!execution.holds(ActionControls::Facing));
        assert!(status.resolved.is_none());
        let mut report = arpg_core::Report::default();
        status.report(&mut report);
        let report = report.finish();
        assert!(report.contains("\"channel_tick\":1"));
        assert!(report.contains("\"action_execution\":{\"elapsed\":1"));
    }
}
