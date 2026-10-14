//! Trees: a scene body on a locked topple joint. Its root is the lock; the
//! fall, what it meets and the log it leaves are physics. What a tree is, how
//! tall a body counts as and what a falling tree's impact does belong here.
use arpg_core::Report;
use arpg_sim::{
    Alpha, EntityId, Fnv, JointConfig, JointSpec, LockState, LockStrength, Mass, ResolvedStep,
    SceneId, Template, ToppleConfig, World, ARENA_HALF,
};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Shared admission and presentation bound across all live scenes.
pub const MAX_TREES: usize = 128;
const _: () = assert!(MAX_TREES <= arpg_sim::MAX_JOINTS);

/// Ground-plane pushes act on a trunk at this height, and bodies are this
/// tall when a falling tree comes down on them.
const CONTACT_HEIGHT: f32 = 1.0;

/// A falling tree closing on a body at least this fast, in m/s at the contact
/// height, damages it. Slower meetings, such as a trunk settling off a
/// support, only push.
const CRUSH_SPEED: f32 = 1.0;
/// Health removed by a damaging strike, whatever the body's allegiance.
const CRUSH_DAMAGE: core::num::NonZeroU8 = core::num::NonZeroU8::new(3).expect("positive damage");

/// The authored woodland mesh catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum TreeKind {
    /// Tiered evergreen.
    Pine,
    /// Rounded leafy canopy.
    Broadleaf,
    /// Leafless branching silhouette.
    Bare,
}

/// Physical trunk of one kind at unit scale. Heights and crowns follow the
/// woodland meshes; width approximates the faceted trunk base, not the canopy.
struct Timber {
    width: f32,
    height: f32,
    crown: f32,
    /// Kilograms; scales with volume.
    mass: f32,
    /// kg m/s per tick at the contact height; scales with trunk cross-section.
    root_impulse: f32,
}
impl TreeKind {
    fn timber(self) -> Timber {
        match self {
            Self::Pine => Timber { width: 0.40, height: 4.35, crown: 1.25, mass: 300.0, root_impulse: 160.0 },
            Self::Broadleaf => Timber { width: 0.48, height: 4.5, crown: 1.6, mass: 360.0, root_impulse: 220.0 },
            Self::Bare => Timber { width: 0.44, height: 3.65, crown: 1.0, mass: 220.0, root_impulse: 120.0 },
        }
    }
}

/// Grounded, metre-scale tree with a validated physical trunk.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "RawTree", into = "RawTree")]
pub struct TreeSpec {
    kind: TreeKind,
    position: Vec2,
    yaw: f32,
    scale: f32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawTree { kind: TreeKind, position: (f32, f32), yaw: f32, scale: f32 }
impl TryFrom<RawTree> for TreeSpec {
    type Error = &'static str;
    fn try_from(raw: RawTree) -> Result<Self, Self::Error> {
        Self::new(raw.kind, raw.position.into(), raw.yaw, raw.scale)
    }
}
impl TreeSpec {
    /// Position is X/Z metres, yaw is radians, scale is uniform in 0.5..=2.
    pub fn new(kind: TreeKind, position: Vec2, yaw: f32, scale: f32) -> Result<Self, &'static str> {
        if !position.is_finite() || position.abs().max_element() > ARENA_HALF {
            return Err("tree position must be finite and within the arena");
        }
        if !yaw.is_finite() || !scale.is_finite() || !(0.5..=2.0).contains(&scale) {
            return Err("tree yaw must be finite and scale within 0.5..=2");
        }
        let spec = Self { kind, position, yaw, scale };
        let (_, trunk, _) = spec.build()?;
        Ok(Self { yaw: trunk.yaw(), ..spec })
    }
    fn build(self) -> Result<(Mass, ToppleConfig, LockStrength), &'static str> {
        let Timber { width, height, crown, mass, root_impulse } = self.kind.timber();
        let s = self.scale;
        let trunk = ToppleConfig::new(width * s, height * s, self.yaw, CONTACT_HEIGHT, crown * s)?;
        if !JointConfig::Topple(trunk).fits_at(self.position) {
            return Err("tree trunk must fit inside the arena");
        }
        Ok((Mass::try_from(mass * s * s * s)?, trunk, LockStrength::try_from(root_impulse * s * s)?))
    }
    /// Physical trunk: base, height and root strength, validated with the placement.
    pub fn trunk(self) -> ToppleConfig {
        self.build().expect("TreeSpec constructor validates the complete trunk").1
    }
    /// The trunk's body and its locked topple joint, bound to scene body `index`.
    pub(crate) fn physical(self, index: usize) -> (arpg_sim::Placed, JointSpec) {
        let (mass, trunk, root) = self.build().expect("TreeSpec constructor validates the complete trunk");
        (
            arpg_sim::Placed { pos: self.position.into(), what: Template::BODY.with_mass(mass) },
            JointSpec { body: index, joint: JointConfig::Topple(trunk), lock: root },
        )
    }
    /// Selected mesh.
    pub fn kind(self) -> TreeKind { self.kind }
    /// Ground-plane position in metres.
    pub fn position(self) -> Vec2 { self.position }
    /// Rotation about world up in radians.
    pub fn yaw(self) -> f32 { self.yaw }
    /// Uniform mesh scale.
    pub fn scale(self) -> f32 { self.scale }
}

/// Observation of a tree's trunk, never a second simulation state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TreeState {
    /// Its root holds it upright.
    Standing,
    /// Its root failed and it is tipping, or leaning where something stopped it.
    Falling,
    /// It lies on the ground as a log.
    Down,
    /// Its trunk body was removed.
    Missing,
}
impl TreeState {
    fn of(joint: Option<arpg_sim::JointState>) -> Self {
        match joint {
            None => Self::Missing,
            Some(j) if j.lock() == LockState::Held => Self::Standing,
            Some(j) if j.grounded() => Self::Down,
            Some(_) => Self::Falling,
        }
    }
}

/// Interpolated mesh placement. Upright trees keep their authored yaw; a
/// falling tree is yawed toward its fall and pitched by its tilt, which turns
/// the canopy about the trunk once, when the root fails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreePose {
    /// Authored placement.
    pub spec: TreeSpec,
    /// Mesh yaw in radians.
    pub yaw: f32,
    /// Tilt about the mesh's local X, toward its local +Z.
    pub pitch: f32,
}

struct Tree {
    owner: SceneId,
    body: EntityId,
    spec: TreeSpec,
}

#[derive(Default)]
pub(crate) struct Trees { rows: Vec<Tree> }
impl Trees {
    pub(crate) fn validate_add(&self, specs: &[TreeSpec]) -> Result<(), crate::SceneError> {
        if specs.len() > MAX_TREES - self.rows.len() {
            return Err(crate::SceneError::Decoration("scene would exceed the live tree limit"));
        }
        Ok(())
    }
    /// `bodies` are the scene's trunk bodies, in authored tree order.
    pub(crate) fn install(&mut self, owner: SceneId, specs: &[TreeSpec], bodies: &[EntityId]) {
        assert_eq!(specs.len(), bodies.len(), "one admitted trunk per tree");
        self.rows.extend(specs.iter().zip(bodies).map(|(&spec, &body)| Tree { owner, body, spec }));
    }
    pub(crate) fn evict(&mut self, owner: SceneId) { self.rows.retain(|row| row.owner != owner); }
    pub(crate) fn iter(&self) -> impl Iterator<Item = TreeSpec> + '_ { self.rows.iter().map(|row| row.spec) }
    fn row(&self, owner: SceneId, nth: usize) -> Option<&Tree> {
        self.rows.iter().filter(|row| row.owner == owner).nth(nth)
    }
    pub(crate) fn body(&self, owner: SceneId, nth: usize) -> Option<EntityId> { self.row(owner, nth).map(|row| row.body) }
    pub(crate) fn state(&self, owner: SceneId, nth: usize, world: &World) -> Option<TreeState> {
        self.row(owner, nth).map(|row| TreeState::of(world.joint(row.body)))
    }
    pub(crate) fn poses<'a>(&'a self, world: &'a World, alpha: Alpha) -> impl Iterator<Item = TreePose> + 'a {
        self.rows.iter().filter_map(move |row| {
            let trunk = world.joint_presentation(row.body, alpha)?;
            Some(match trunk.heading() {
                None => TreePose { spec: row.spec, yaw: row.spec.yaw, pitch: 0.0 },
                Some(yaw) => TreePose { spec: row.spec, yaw, pitch: trunk.angle() },
            })
        })
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for Tree { owner, body, spec } in &self.rows {
            owner.hash_into(out);
            body.hash_into(out);
            out.usize(match spec.kind { TreeKind::Pine => 0, TreeKind::Broadleaf => 1, TreeKind::Bare => 2 });
            for value in [spec.position.x, spec.position.y, spec.yaw, spec.scale] { out.f32(value); }
        }
    }
    pub(crate) fn report(&self, world: &World, out: &mut Report) {
        out.int("tree_count", self.rows.len() as u64);
        out.object("trees", |out| {
            for scene in self.rows.chunk_by(|a, b| a.owner == b.owner) {
                for (nth, Tree { owner, body, spec }) in scene.iter().enumerate() {
                    out.object(&format!("{owner}/{nth}"), |out| {
                        out.text("body", &body.to_string());
                        out.text("scene", &owner.to_string());
                        out.text("kind", match spec.kind { TreeKind::Pine => "Pine", TreeKind::Broadleaf => "Broadleaf", TreeKind::Bare => "Bare" });
                        out.vec3("position", glam::Vec3::new(spec.position.x, 0.0, spec.position.y));
                        out.num("yaw", spec.yaw);
                        out.num("scale", spec.scale);
                        let state = self.state(*owner, nth, world).expect("resident tree");
                        out.text("state", &format!("{state:?}"));
                    });
                }
            }
        });
    }
}

impl Trees {
    /// Damage every body a falling tree closed on fast enough this tick. Runs
    /// after the player's attack and before actor effects; health and removal
    /// rules refuse bodies already defeated, immune or gone.
    pub(crate) fn crush(&self, resolved: &mut ResolvedStep<'_>) {
        for impact in 0..resolved.impacts().len() {
            let record = resolved.impacts()[impact];
            if record.closing_speed() >= CRUSH_SPEED && self.rows.iter().any(|row| row.body == record.source()) {
                let _ = resolved.damage_impact(impact, CRUSH_DAMAGE);
            }
        }
    }
}

impl From<TreeSpec> for RawTree {
    fn from(tree: TreeSpec) -> Self { Self { kind: tree.kind, position: tree.position.into(), yaw: tree.yaw, scale: tree.scale } }
}
