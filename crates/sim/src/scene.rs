//! Authored content and the lifetime of one instantiation of it.
//!
//! Loading is synchronous between ticks in this first slice. Validation and
//! capacity admission precede every mutation, so success means fully installed
//! and failure leaves the world untouched. Streaming policy and file I/O do
//! not belong here. Runtime instances own ids, never body storage.

use core::fmt;

use arpg_core::Report;
use glam::Vec2;
use serde::Deserialize;

use crate::{Condition, EntityId, Fnv, SourceId, SourceSpec, Template};

/// One authored body, independent of its eventual runtime identity.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placed {
    /// World-space ground position `(x, z)`.
    pub pos: (f32, f32),
    /// Behaviours granted through the ordinary spawn implementation.
    #[serde(default)]
    pub what: Template,
}

/// A reusable description. Loading it twice creates two independent instances.
/// The player belongs to the world, not to this disposable content.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// Human-readable content label; never used as runtime identity.
    pub name: String,
    /// Authored order is spawn order, and therefore deterministic id order.
    #[serde(default)]
    pub bodies: Vec<Placed>,
    /// Descriptions, not running sources: every load starts their cadence anew.
    #[serde(default)]
    pub sources: Vec<SourceSpec>,
}

impl Scene {
    /// The familiar startup horde, as content that uses the normal scene loader.
    /// Constructed in code so launching the binary never depends on its cwd.
    #[must_use]
    pub fn boot() -> Self {
        Self {
            name: "Default horde".into(),
            bodies: crate::grid_positions(crate::DEFAULT_ENEMIES)
                .map(|pos| Placed { pos: pos.into(), what: Template::BODY })
                .collect(),
            sources: Vec::new(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), SceneError> {
        let Self { name: _, bodies, sources } = self;
        if bodies.iter().any(|body| !Vec2::from(body.pos).is_finite()) {
            return Err(SceneError::Invalid("body position must be finite"));
        }
        for source in sources {
            let SourceSpec { pos, radius, every: _, when, what: _ } = source;
            if !Vec2::from(*pos).is_finite()
                || !radius.is_finite()
                || *radius < 0.0
                || !(Vec2::from(*pos).abs() + Vec2::splat(*radius)).is_finite()
            {
                return Err(SceneError::Invalid(
                    "source position/radius must be finite; radius must be nonnegative",
                ));
            }
            if let Condition::PlayerWithin(r) = when
                && (!r.is_finite() || *r < 0.0)
            {
                return Err(SceneError::Invalid("source proximity must be finite and nonnegative"));
            }
        }
        Ok(())
    }
}

/// The identity of one live instantiation, scoped to its World.
/// Monotonic and never reused in that world; removing records does not rewind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneId(u64);

impl SceneId {
    /// Reads the same form emitted by the trace and state report, such as `c0`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.strip_prefix('c')?.parse().ok().map(Self)
    }

    pub(crate) fn hash(self, hash: &mut Fnv) {
        hash.u64(self.0);
    }
}

impl fmt::Display for SceneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c{}", self.0)
    }
}

/// A rejected instantiation. No bodies, sources, ids, or trace events changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneError {
    /// The content contains invalid simulation values.
    Invalid(&'static str),
    /// Authored bodies plus already accepted spawns exceed world capacity.
    Capacity,
    /// The world's monotonically increasing scene or source identities are exhausted.
    IdExhausted,
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => f.write_str(reason),
            Self::Capacity => {
                f.write_str("scene exceeds available body capacity (including queued spawns)")
            }
            Self::IdExhausted => f.write_str("scene or source identities exhausted"),
        }
    }
}

impl std::error::Error for SceneError {}

/// Only live records are retained. A packed id list costs the owned population,
/// unlike one slot-indexed sparse array per scene. Individual deaths remove ids
/// eagerly; eviction takes the whole record before walking it.
struct Resident {
    id: SceneId,
    name: String,
    bodies: Vec<EntityId>,
    sources: Vec<SourceId>,
}

#[derive(Default)]
pub(crate) struct Scenes {
    live: Vec<Resident>,
    next: u64,
}

impl Scenes {
    fn can_add(&self) -> Result<(), SceneError> {
        self.next.checked_add(1).map(|_| ()).ok_or(SceneError::IdExhausted)
    }

    fn add(&mut self, name: &str) -> SceneId {
        let id = SceneId(self.next);
        self.next = self.next.checked_add(1).expect("scene admission checked identity capacity");
        self.live.push(Resident { id, name: name.into(), bodies: Vec::new(), sources: Vec::new() });
        id
    }

    fn get(&self, id: SceneId) -> Option<&Resident> {
        self.live.iter().find(|scene| scene.id == id)
    }

    pub(crate) fn contains(&self, id: SceneId) -> bool {
        self.get(id).is_some()
    }

    pub(crate) fn record_body(&mut self, owner: SceneId, body: EntityId) {
        self.live
            .iter_mut()
            .find(|scene| scene.id == owner)
            .expect("only a live owner may receive a body")
            .bodies
            .push(body);
    }

    fn record_source(&mut self, owner: SceneId, source: SourceId) {
        self.live
            .iter_mut()
            .find(|scene| scene.id == owner)
            .expect("only a live owner may receive a source")
            .sources
            .push(source);
    }

    pub(crate) fn forget_body(&mut self, body: EntityId) {
        for scene in &mut self.live {
            scene.bodies.retain(|id| *id != body);
        }
    }

    pub(crate) fn forget_source(&mut self, source: SourceId) {
        for scene in &mut self.live {
            scene.sources.retain(|id| *id != source);
        }
    }

    pub(crate) fn clear_bodies(&mut self) {
        for scene in &mut self.live {
            scene.bodies.clear();
        }
    }

    fn take(&mut self, id: SceneId) -> Option<Resident> {
        let row = self.live.iter().position(|scene| scene.id == id)?;
        Some(self.live.remove(row))
    }

    pub(crate) fn len(&self) -> usize {
        self.live.len()
    }

    pub(crate) fn hash(&self, hash: &mut Fnv) {
        let Self { live, next } = self;
        hash.u64(*next);
        hash.usize(live.len());
        for scene in live {
            let Resident { id, name, bodies, sources } = scene;
            id.hash(hash);
            hash.usize(name.len());
            for byte in name.bytes() {
                hash.u64(u64::from(byte));
            }
            hash.usize(bodies.len());
            for body in bodies {
                body.hash_into(hash);
            }
            hash.usize(sources.len());
            for source in sources {
                source.hash(hash);
            }
        }
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { live, next } = self;
        out.int("next_id", *next);
        for scene in live {
            let Resident { id, name, bodies, sources } = scene;
            out.object(&id.to_string(), |out| {
                out.text("name", name);
                out.int("bodies", bodies.len() as u64);
                out.int("sources", sources.len() as u64);
                out.object("members", |out| {
                    for body in bodies {
                        out.bool(&body.to_string(), true);
                    }
                });
                out.object("source_ids", |out| {
                    for source in sources {
                        out.bool(&source.to_string(), true);
                    }
                });
            });
        }
    }
}

impl crate::World {
    /// Builds a repeatable playtest at tick zero, using the same instantiation
    /// door as additive loading. No previous player, tuning, or source survives.
    pub fn from_scene(scene: &Scene) -> Result<Self, SceneError> {
        let mut world = Self::empty();
        world.load_scene(scene)?;
        Ok(world)
    }

    /// Installs an entire disposable scene between ticks. A successful return
    /// means ready now; no hidden ticks run and there are no partial loads.
    /// Existing accepted spawns retain their capacity reservation.
    ///
    /// Like `place`, this needs `&mut World`, which no simulation pass receives.
    /// Future streaming decisions must request a structural boundary operation,
    /// rather than obtaining this authority inside the schedule.
    pub fn load_scene(&mut self, scene: &Scene) -> Result<SceneId, SceneError> {
        scene.validate()?;
        let available =
            crate::MAX_ENEMIES.saturating_sub(self.enemy_count()).saturating_sub(self.queue.len());
        if scene.bodies.len() > available {
            return Err(SceneError::Capacity);
        }
        self.scenes.can_add()?;
        if !self.sources.can_add(scene.sources.len()) {
            return Err(SceneError::IdExhausted);
        }

        let owner = self.scenes.add(&scene.name);
        for body in &scene.bodies {
            let id = self
                .place(Vec2::from(body.pos), body.what)
                .expect("scene admission reserved every authored body");
            self.scenes.record_body(owner, id);
        }
        for source in &scene.sources {
            let id = self.sources.add_owned((*source).into(), Some(owner));
            self.scenes.record_source(owner, id);
            self.trace.sink(self.tick).emit(crate::Event::SourceAdded { id });
        }
        self.trace.sink(self.tick).emit(crate::Event::SceneLoaded { id: owner });
        Ok(owner)
    }

    /// Removes this instance, its source descendants, and its queued emissions.
    /// Player and other instances survive. A retired id is an ordinary no-op.
    /// Destruction completes between ticks, before sources can next evaluate.
    pub fn evict_scene(&mut self, id: SceneId) -> bool {
        let Some(resident) = self.scenes.take(id) else {
            return false;
        };
        self.queue.cancel_scene(id);
        for source in resident.sources {
            self.remove_source(source);
        }
        for body in resident.bodies {
            self.despawn_enemy(body);
        }
        self.trace.sink(self.tick).emit(crate::Event::SceneEvicted { id });
        true
    }

    /// Number of completely installed scene instances.
    #[must_use]
    pub fn scene_count(&self) -> usize {
        self.scenes.len()
    }

    /// Ready instances in creation order, with their authored labels.
    pub fn scene_instances(&self) -> impl Iterator<Item = (SceneId, &str)> {
        self.scenes.live.iter().map(|scene| (scene.id, scene.name.as_str()))
    }

    /// Live bodies owned by an instance, including its source descendants.
    /// The shared slice cannot change ownership; absent means evicted/unknown.
    #[must_use]
    pub fn scene_bodies(&self, id: SceneId) -> Option<&[EntityId]> {
        self.scenes.get(id).map(|scene| scene.bodies.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Accumulator, Dt, Impulse, World};
    use arpg_core::Intent;

    fn scene() -> Scene {
        Scene {
            name: "pair".into(),
            bodies: vec![Placed { pos: (20.0, 0.0), what: Template::BODY }],
            sources: vec![SourceSpec {
                pos: (30.0, 0.0),
                radius: 4.0,
                every: 2,
                when: Condition::Always,
                what: Template::BODY,
            }],
        }
    }

    fn step(world: &mut World) {
        for dt in Accumulator::default().pending(Dt::SECS) {
            world.step(dt, Intent::default());
        }
    }

    #[test]
    fn eviction_cancels_owned_work_and_preserves_other_instances() {
        let mut world = World::empty();
        let a = world.load_scene(&scene()).unwrap();
        let b = world.load_scene(&scene()).unwrap();
        step(&mut world);
        let old = world.scene_bodies(a).unwrap().to_vec();
        let survivors = world.scene_bodies(b).unwrap().to_vec();
        assert_eq!(old.len(), 2, "the source descendant must inherit ownership");
        assert!(world.queue.push_owned(Vec2::new(50.0, 0.0), Template::BODY, Some(a)));
        assert!(world.request_spawn(Vec2::new(-50.0, 0.0), Template::BODY));
        assert!(world.evict_scene(a));
        assert!(!world.evict_scene(a));
        assert_eq!(world.queue.len(), 1, "unowned queued work must survive");
        for _ in 0..4 {
            step(&mut world);
        }
        assert!(old.iter().all(|id| !world.is_alive(*id)));
        assert!(survivors.iter().all(|id| world.is_alive(*id)));
        assert_eq!(world.source_count(), 1);
        assert_eq!(world.enemy_count(), 5, "B's four bodies plus one unowned body");
        let again = world.load_scene(&scene()).unwrap();
        assert_ne!(a, again);
    }

    #[test]
    fn stale_producer_cannot_resurrect_an_evicted_scene() {
        let mut world = World::empty();
        let a = world.load_scene(&Scene::default()).unwrap();
        world.evict_scene(a);
        assert!(world.queue.push_owned(Vec2::ZERO, Template::BODY, Some(a)));
        step(&mut world);
        assert_eq!(world.enemy_count(), 0);
        assert!(world.trace().iter().any(|(_, e)| e == crate::Event::Refused { count: 1 }));
    }

    #[test]
    fn recycled_body_slot_and_bulk_reset_do_not_leave_ownership() {
        let mut world = World::empty();
        let a = world.load_scene(&scene()).unwrap();
        let old = world.scene_bodies(a).unwrap()[0];
        world.despawn_enemy(old);
        let survivor = world.place(Vec2::new(60.0, 0.0), Template::BODY).unwrap();
        assert_eq!(world.scene_bodies(a).unwrap().len(), 0);
        world.evict_scene(a);
        assert!(world.is_alive(survivor));
        let b = world.load_scene(&scene()).unwrap();
        world.set_enemy_count(2);
        assert!(world.scene_bodies(b).unwrap().is_empty());
        world.evict_scene(b);
        assert_eq!(world.enemy_count(), 2);
    }

    #[test]
    fn invalid_and_over_capacity_scenes_leave_everything_untouched() {
        let mut world = World::empty();
        let mut invalid = scene();
        invalid.bodies.push(Placed { pos: (f32::NAN, 0.0), what: Template::BODY });
        let before = world.hash();
        let trace = world.trace().render();
        assert!(matches!(world.load_scene(&invalid), Err(SceneError::Invalid(_))));
        assert_eq!(world.hash(), before);
        assert_eq!(world.trace().render(), trace);
        let full = Scene {
            name: "full".into(),
            bodies: vec![Placed { pos: (1.0, 0.0), what: Template::BODY }; crate::MAX_ENEMIES],
            sources: vec![],
        };
        assert!(world.request_spawn(Vec2::ZERO, Template::BODY));
        let before = world.hash();
        assert_eq!(world.load_scene(&full), Err(SceneError::Capacity));
        assert_eq!(world.hash(), before, "accepted queue capacity is reserved too");
    }

    #[test]
    fn invalid_sources_are_rejected_before_any_scene_content_is_installed() {
        for (pos, radius, when) in [
            ((f32::INFINITY, 0.0), 0.0, Condition::Always),
            ((0.0, 0.0), -1.0, Condition::Always),
            ((0.0, 0.0), f32::NAN, Condition::Always),
            ((f32::MAX, 0.0), f32::MAX, Condition::Always),
            ((0.0, 0.0), 0.0, Condition::PlayerWithin(-1.0)),
            ((0.0, 0.0), 0.0, Condition::PlayerWithin(f32::NAN)),
        ] {
            let mut world = World::from_scene(&scene()).unwrap();
            let before = world.hash();
            let trace = world.trace().render();
            let mut invalid = scene();
            invalid.sources.push(SourceSpec { pos, radius, every: 1, when, what: Template::BODY });
            assert!(matches!(world.load_scene(&invalid), Err(SceneError::Invalid(_))));
            assert_eq!(
                world.hash(),
                before,
                "even the valid preceding body/source must stay uninstalled"
            );
            assert_eq!(world.trace().render(), trace);
        }
    }

    #[test]
    fn fresh_playtests_reset_player_tuning_time_and_content() {
        let content = scene();
        let mut played = World::from_scene(&content).unwrap();
        assert!(played.apply_impulse(played.player_id(), Impulse::try_from((6.0, 0.0)).unwrap()));
        played.set_attack_recovery(crate::RecoveryTicks::try_from(1).unwrap());
        step(&mut played);
        let fresh = World::from_scene(&content).unwrap();
        assert_ne!(played.hash(), fresh.hash());
        played = World::from_scene(&content).unwrap();
        assert_eq!(played.hash(), fresh.hash());
        assert_eq!(played.tick(), 0);
        assert_eq!(played.enemy_count(), 1);
        assert_eq!(played.motion(played.player_id()).unwrap().velocity(), Vec2::ZERO);
    }

    #[test]
    fn scene_state_and_pending_ownership_reach_the_hash() {
        type Change = fn(&mut World);
        let fields: &[(&str, Change)] = &[
            ("next identity", |w| w.scenes.next += 1),
            ("name", |w| w.scenes.live[0].name.push('x')),
            ("bodies", |w| w.scenes.live[0].bodies.clear()),
            ("sources", |w| w.scenes.live[0].sources.clear()),
        ];
        for (field, change) in fields {
            let mut world = World::from_scene(&scene()).unwrap();
            let before = world.hash();
            change(&mut world);
            assert_ne!(world.hash(), before, "{field} missing from hash");
        }
        let mut a = World::from_scene(&scene()).unwrap();
        let mut b = World::from_scene(&scene()).unwrap();
        let owner = a.scene_instances().next().unwrap().0;
        assert!(a.queue.push_owned(Vec2::ZERO, Template::BODY, Some(owner)));
        assert!(b.queue.push_owned(Vec2::ZERO, Template::BODY, None));
        assert_ne!(a.hash(), b.hash(), "queued ownership changes future eviction");
    }

    #[test]
    fn boot_is_ready_without_queue_limits_or_interpolation_streaks() {
        const {
            assert!(crate::DEFAULT_ENEMIES > crate::pass::spawn::QUEUE_CAPACITY);
        }
        let world = World::from_scene(&Scene::boot()).unwrap();
        assert_eq!(world.enemy_count(), crate::DEFAULT_ENEMIES);
        assert_eq!(world.tick(), 0);
        assert_eq!(world.queue.len(), 0);
        assert_eq!(world.bodies.pos, world.bodies.prev_pos);
        let legacy = World::default();
        assert_eq!(world.bodies.pos, legacy.bodies.pos, "boot keeps the established layout");
    }
}
