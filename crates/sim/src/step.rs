//! Borrowed tick phases keep structural admission before action decisions and
//! final defeat removal after all explicit effects. A phase must be consumed.
use crate::{
    ActorView, Dt, EntityId, Event, MotorCommand, SpawnOutcome, StepObserver, World, pass,
};
use core::num::NonZeroU8;

/// Births are visible; gameplay chooses motors from the shared start-of-tick pose.
#[must_use = "advance and finish the admitted tick"]
pub struct PreparedStep<'a> {
    world: Option<&'a mut World>,
    dt: Dt,
}
/// Physics has resolved; gameplay may apply explicit effects.
#[must_use = "finish the tick after resolving gameplay effects"]
pub struct ResolvedStep<'a> {
    world: Option<&'a mut World>,
}

impl World {
    /// Admits all births for this tick before gameplay binds and evaluates actions.
    /// Population is supplied by gameplay, independently of body/health counts.
    pub fn begin_step(&mut self, dt: Dt, population: usize) -> PreparedStep<'_> {
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
            &self.bodies.pos,
            &mut self.bodies.prev_pos,
        );
        PreparedStep {
            world: Some(self),
            dt,
        }
    }
}
impl<'a> PreparedStep<'a> {
    /// Physical facts after admission, before any actor moves.
    pub fn view(&self) -> ActorView<'_> {
        self.world.as_ref().expect("prepared tick").actor_view()
    }
    /// Lossless receipts include source attribution and owning scene.
    pub fn spawn_outcomes(&self) -> &[SpawnOutcome] {
        self.world.as_ref().expect("prepared tick").spawn_outcomes()
    }
    /// Apply all actor motors and resolve shared physics before gameplay effects.
    pub fn advance<'m>(mut self, motors: impl IntoIterator<Item = &'m MotorCommand>, observer: &mut impl StepObserver) -> ResolvedStep<'a> {
        let world = self.world.take().expect("prepared tick");
        let dt = self.dt;
        for command in motors {
            if world.actor_view().movable(command.actor) {
                world.bodies.physics.drive().toward(command.actor, command.velocity, dt, command.acceleration);
            }
        }
        let mut trace = world.trace.sink(world.tick);
        observer.crowd_started();
        let (crowd_work, contacts) = crate::solver::solve(
            crate::solver::Geometry {
                ids: world.bodies.slots.ids(),
                walls: &world.scenes.walls,
                material: world.material,
            },
            &mut world.bodies.pos,
            &mut world.bodies.physics,
            &mut world.crowd_grid,
            dt,
            trace.reborrow(),
            observer,
        );
        world.crowd_contacts = crowd_work.contacts();
        world.contacts = contacts;
        observer.crowd_finished(crowd_work);
        pass::motion::settle(&mut world.bodies.physics, dt, trace.reborrow());
        // **After the contact solve and settle.** An unlocked pole tips with
        // the load its base absorbed this tick and meets bodies at their final
        // positions. Like attack impulses, an impact changes velocity now and
        // moves its body on the next physics step.
        pass::topple::topple(
            &mut world.bodies.physics,
            world.bodies.slots.ids(),
            &world.bodies.pos,
            &world.scenes.walls,
            &mut world.crowd_grid,
            dt,
            trace.reborrow(),
        );
        ResolvedStep { world: Some(world) }
    }
}
impl ResolvedStep<'_> {
    /// Apply physical impact independently of damage eligibility.
    pub fn impact(
        &mut self,
        actor: EntityId,
        target: EntityId,
        point: glam::Vec2,
        impulse: crate::Impulse,
    ) -> bool {
        let world = self.world.as_mut().expect("resolved tick");
        if actor == target
            || !world.actor_view().can_act(actor)
            || world.actor_view().position(target).is_none()
        {
            return false;
        }
        world.bodies.physics.impulse_at(
            target,
            point,
            impulse,
            Some(actor),
            &mut world.trace.sink(world.tick),
        )
    }
    /// Final positions and current health while explicit effects resolve.
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
        world.hit(actor, target, amount)
    }
    /// Bodies met by moving jointed bodies during this tick's physics.
    pub fn impacts(&self) -> &[crate::Impact] {
        &self.world.as_ref().expect("resolved tick").bodies.physics.joints.impacts
    }
    /// Apply positive damage for one of this tick's impacts. Its source is
    /// the damage's actor; each impact damages at most once, and a target
    /// already defeated or removed is refused.
    pub fn damage_impact(&mut self, impact: usize, amount: NonZeroU8) -> Option<u8> {
        let world = self.world.as_mut().expect("resolved tick");
        let record = *world.bodies.physics.joints.impacts.get(impact)?;
        if record.damaged || !world.actor_view().alive(record.body) {
            return None;
        }
        let remaining = world.hit(record.source, record.body, amount)?;
        world.bodies.physics.joints.impacts[impact].damaged = true;
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
            &world.bodies.physics,
            actor,
            target,
            &mut world.trace.sink(world.tick),
        )
    }
    /// Activate the nearest eligible endpoint using shared reach and cover admission.
    pub fn interact_nearest(&mut self, actor: EntityId) {
        let world = self.world.as_mut().expect("resolved tick");
        pass::interact::interact(&mut world.bodies.interactions, &world.bodies.slots, &world.bodies.pos, actor, true, &world.bodies.health, &world.scenes.walls, &world.bodies.physics, world.trace.sink(world.tick));
    }
    /// Publish death immediately; moving characters retire once their physical motion rests.
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
impl World {
    /// Remove health and trace the hit; callers own eligibility.
    fn hit(&mut self, actor: EntityId, target: EntityId, amount: NonZeroU8) -> Option<u8> {
        let remaining = self.bodies.health.hit(target, amount)?;
        self.trace.sink(self.tick).emit(Event::Damaged { actor, target, remaining });
        Some(remaining)
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
