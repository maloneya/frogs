//! Borrowed tick phases keep structural admission before action decisions and
//! final defeat removal after all explicit effects. A phase must be consumed.
use crate::{
    ActorView, Dt, EntityId, Event, MotorCommand, SpawnOutcome, StepObserver, World, pass,
};
use arpg_core::Intent;
use core::num::NonZeroU8;

/// Births and player movement are visible; gameplay may now choose action motors.
#[must_use = "advance and finish the admitted tick"]
pub struct PreparedStep<'a> {
    world: Option<&'a mut World>,
    dt: Dt,
    intent: Intent,
}
/// Physics and the player's attack have resolved; gameplay may apply explicit effects.
#[must_use = "finish the tick after resolving gameplay effects"]
pub struct ResolvedStep<'a> {
    world: Option<&'a mut World>,
}

impl World {
    /// Admits all births for this tick before gameplay binds and evaluates actions.
    /// Population is supplied by gameplay, independently of body/health counts.
    pub fn begin_step(&mut self, dt: Dt, intent: Intent, population: usize) -> PreparedStep<'_> {
        let move_dir = pass::beam::movement(&self.player.attack, intent);
        let mut trace = self.trace.sink(self.tick);
        pass::source::trigger(
            &mut self.sources,
            &mut self.queue,
            self.bodies.pos[0],
            population,
            trace.reborrow(),
        );
        pass::spawn::drain(
            &mut self.queue,
            &mut self.bodies,
            &mut self.scenes,
            trace.reborrow(),
        );
        pass::remember::remember(
            self.player.facing,
            &mut self.player.prev_facing,
            &self.bodies.pos,
            &mut self.bodies.prev_pos,
        );
        pass::walk::walk(
            &mut self.bodies.pos[0],
            move_dir,
            dt,
            self.tuning.get(crate::TuningField::PlayerSpeed),
        );
        PreparedStep {
            world: Some(self),
            dt,
            intent,
        }
    }
}
impl<'a> PreparedStep<'a> {
    /// Physical facts after admission and controlled player movement.
    pub fn view(&self) -> ActorView<'_> {
        self.world.as_ref().expect("prepared tick").actor_view()
    }
    /// Lossless receipts include source attribution and owning scene.
    pub fn spawn_outcomes(&self) -> &[SpawnOutcome] {
        self.world.as_ref().expect("prepared tick").spawn_outcomes()
    }
    /// Apply one motor per actor, shared physics, then player attack effects.
    /// Target identities are a gameplay decision; only those identities can be hit.
    pub fn advance(
        mut self,
        motors: &[MotorCommand],
        player_targets: &[EntityId],
        observer: &mut impl StepObserver,
    ) -> ResolvedStep<'a> {
        let world = self.world.as_mut().expect("prepared tick");
        world.player_targets.clear();
        for &id in player_targets {
            if id != world.player_id() {
                world.player_targets.add(id);
            }
        }
        self.advance_inner(motors, observer)
    }
    pub(crate) fn advance_standalone(
        mut self,
        observer: &mut impl StepObserver,
    ) -> ResolvedStep<'a> {
        // Include births admitted by this tick in the standalone target set.
        let world = self.world.as_mut().expect("prepared tick");
        let player = world.player_id();
        for &id in world.bodies.health.ids() {
            if id != player {
                world.player_targets.add(id);
            }
        }
        self.advance_inner(&[], observer)
    }
    fn advance_inner(
        mut self,
        motors: &[MotorCommand],
        observer: &mut impl StepObserver,
    ) -> ResolvedStep<'a> {
        let world = self.world.take().expect("prepared tick");
        let dt = self.dt;
        let intent = self.intent;
        for command in motors {
            if world.actor_view().movable(command.actor) {
                world.bodies.physics.drive().toward(
                    command.actor,
                    command.velocity,
                    dt,
                    command.acceleration,
                );
            }
        }
        let mut trace = world.trace.sink(world.tick);
        // Physical velocity moves before contacts. Crowd resolution precedes
        // player resolution, retaining the existing crowd priority.
        pass::separate::prevent_crossings(
            &world.bodies.pos,
            world.bodies.slots.ids(),
            &mut world.bodies.physics,
            &world.scenes.walls,
            dt,
            trace.reborrow(),
        );
        pass::motion::integrate(
            &world.bodies.physics,
            &world.bodies.slots,
            &mut world.bodies.pos,
            dt,
        );
        pass::walls::resolve(
            &world.scenes.walls,
            &world.bodies.prev_pos,
            &mut world.bodies.pos,
            world.bodies.slots.ids(),
            &mut world.bodies.physics,
            trace.reborrow(),
        );
        observer.crowd_started();
        let crowd_work = pass::separate::crowd(
            &mut world.bodies.pos[1..],
            &world.bodies.slots.ids()[1..],
            &mut world.bodies.physics,
            &mut world.crowd_grid,
            trace.reborrow(),
        );
        world.crowd_contacts = crowd_work.contacts();
        observer.crowd_finished(crowd_work);
        world.contacts = pass::separate::player(
            &mut world.bodies.pos,
            world.bodies.slots.ids(),
            &mut world.bodies.physics,
            trace.reborrow(),
        );
        pass::contain::contain(
            &mut world.bodies.pos,
            world.bodies.slots.ids(),
            &mut world.bodies.physics,
            trace.reborrow(),
        );
        pass::walls::resolve(
            &world.scenes.walls,
            &world.bodies.prev_pos,
            &mut world.bodies.pos,
            world.bodies.slots.ids(),
            &mut world.bodies.physics,
            trace.reborrow(),
        );
        pass::motion::settle(&mut world.bodies.physics, dt, trace.reborrow());
        pass::face::aim(&mut world.player.facing, &world.player.attack, intent, dt);

        // **After `face`.** The hitbox is oriented by the facing this
        // tick ended with, and it is tested against where the bodies actually
        // ended up — after motors, after both solvers, after the wall. Running
        // it earlier would swing at positions that no longer exist by the time
        // the tick is over.
        pass::attack::attack(
            &mut world.player.attack,
            pass::attack::Pose {
                actor: world.bodies.slots.ids()[0],
                pos: world.bodies.pos[0],
                facing: world.player.facing,
            },
            intent,
            &world.bodies.slots.ids()[1..],
            &world.bodies.pos[1..],
            world.bodies.physics.sink(world.bodies.slots.ids()[0]),
            world.bodies.health.sink_for(&world.player_targets),
            &world.scenes.walls,
            trace.reborrow(),
        );
        pass::interact::interact(
            &mut world.bodies.interactions,
            &world.bodies.slots,
            &world.bodies.pos,
            world.bodies.slots.ids()[0],
            intent.interact(),
            &world.bodies.health,
            &world.scenes.walls,
            trace.reborrow(),
        );
        ResolvedStep { world: Some(world) }
    }
}
impl ResolvedStep<'_> {
    /// Final positions and health after physics and the player's attack.
    pub fn view(&self) -> ActorView<'_> {
        self.world.as_ref().expect("resolved tick").actor_view()
    }
    /// Apply positive damage from an eligible actor to a live target. Repeated lethal
    /// hits and stale actors/targets are refused without manufacturing effects.
    pub fn damage(&mut self, actor: EntityId, target: EntityId, amount: NonZeroU8) -> Option<u8> {
        let world = self.world.as_mut().expect("resolved tick");
        if actor == target
            || !world.actor_view().can_act(actor)
            || !world.actor_view().alive(target)
        {
            return None;
        }
        let remaining = world.bodies.health.sink().hit(target, amount)?;
        world.trace.sink(world.tick).emit(Event::Damaged {
            actor,
            target,
            remaining,
        });
        Some(remaining)
    }
    /// Activate an explicit ready endpoint after checking live actor, reach and cover.
    pub fn interact(&mut self, actor: EntityId, target: EntityId) -> bool {
        let world = self.world.as_mut().expect("resolved tick");
        pass::interact::activate(
            &mut world.bodies.interactions,
            &world.bodies.slots,
            &world.bodies.pos,
            &world.bodies.health,
            &world.scenes.walls,
            actor,
            target,
            &mut world.trace.sink(world.tick),
        )
    }
    /// Retire defeated nonplayer bodies and publish the completed tick.
    pub fn finish(mut self) {
        let world = self.world.take().expect("resolved tick");
        pass::health::remove_defeated(
            &mut world.bodies,
            &mut world.scenes,
            &mut world.defeats,
            world.trace.sink(world.tick),
        );
        world.tick += 1;
    }
}
impl Drop for PreparedStep<'_> {
    fn drop(&mut self) {
        assert!(
            self.world.is_none() || std::thread::panicking(),
            "an admitted tick must be advanced and finished"
        );
    }
}
impl Drop for ResolvedStep<'_> {
    fn drop(&mut self) {
        assert!(
            self.world.is_none() || std::thread::panicking(),
            "a resolved tick must be finished"
        );
    }
}
