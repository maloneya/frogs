//! Scene-owned trees with trunk geometry installed through shared static collision.
use arpg_core::Report;
use arpg_sim::{Fnv, SceneId, ARENA_HALF};
use glam::Vec2;
use serde::Deserialize;

/// Shared admission and presentation bound across all live scenes.
pub const MAX_TREES: usize = 128;

/// The authored woodland mesh catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TreeKind {
    /// Tiered evergreen.
    Pine,
    /// Rounded leafy canopy.
    Broadleaf,
    /// Leafless branching silhouette.
    Bare,
}

/// Grounded, metre-scale tree with a validated solid trunk footprint.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(try_from = "RawTree")]
pub struct TreeSpec {
    kind: TreeKind,
    position: Vec2,
    yaw: f32,
    scale: f32,
}

#[derive(Deserialize)]
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
        let collider = spec.build_collider()?;
        Ok(Self { yaw: collider.yaw(), ..spec })
    }
    fn build_collider(self) -> Result<arpg_sim::Wall, &'static str> {
        // Small boxes approximate the faceted trunk bases, not the canopy.
        // The same scaled, rotated shape supplies motion and attack obstruction.
        let half_extent = match self.kind {
            TreeKind::Pine => 0.20,
            TreeKind::Broadleaf => 0.24,
            TreeKind::Bare => 0.22,
        };
        arpg_sim::Wall::new(self.position, Vec2::splat(2.0 * half_extent * self.scale), self.yaw)
    }
    /// Solid ground-plane trunk, validated together with the mesh placement.
    pub fn collider(self) -> arpg_sim::Wall {
        self.build_collider().expect("TreeSpec constructor validates the complete footprint")
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

struct Tree {
    owner: SceneId,
    wall_index: usize,
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
    pub(crate) fn install(&mut self, owner: SceneId, first_wall: usize, specs: &[TreeSpec]) {
        self.validate_add(specs).expect("validated before physical admission");
        self.rows.extend(specs.iter().enumerate().map(|(nth, &spec)| Tree { owner, wall_index: first_wall + nth, spec }));
    }
    pub(crate) fn evict(&mut self, owner: SceneId) { self.rows.retain(|row| row.owner != owner); }
    pub(crate) fn iter(&self) -> impl Iterator<Item = TreeSpec> + '_ { self.rows.iter().map(|row| row.spec) }
    pub(crate) fn owns_wall(&self, owner: SceneId, wall_index: usize) -> bool {
        self.rows.iter().any(|row| row.owner == owner && row.wall_index == wall_index)
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for Tree { owner, wall_index, spec } in &self.rows {
            owner.hash_into(out);
            out.usize(*wall_index);
            out.usize(match spec.kind { TreeKind::Pine => 0, TreeKind::Broadleaf => 1, TreeKind::Bare => 2 });
            for value in [spec.position.x, spec.position.y, spec.yaw, spec.scale] { out.f32(value); }
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.int("tree_count", self.rows.len() as u64);
        out.object("trees", |out| {
            for scene in self.rows.chunk_by(|a, b| a.owner == b.owner) {
                for (nth, Tree { owner, wall_index, spec }) in scene.iter().enumerate() {
                    out.object(&format!("{owner}/{nth}"), |out| {
                        out.int("collider_index", *wall_index as u64);
                        out.text("scene", &owner.to_string());
                        out.text("kind", match spec.kind { TreeKind::Pine => "Pine", TreeKind::Broadleaf => "Broadleaf", TreeKind::Bare => "Bare" });
                        out.vec3("position", glam::Vec3::new(spec.position.x, 0.0, spec.position.y));
                        out.num("yaw", spec.yaw);
                        out.num("scale", spec.scale);
                    });
                }
            }
        });
    }
}
