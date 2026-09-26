//! Static ground-plane rectangles shared by motion and obstruction queries.
//! Height is deliberately absent: it belongs to presentation.
use crate::{ARENA_HALF, Fnv, SceneError, SceneId};
use arpg_core::Report;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Admission bound for live wall rectangles, independent of circular bodies.
pub const MAX_WALLS: usize = 256;
/// Separation margin in metres, small compared with the minimum wall thickness.
const SKIN: f32 = 0.00001;
/// Bounded work when a sweep meets several faces. Unspent motion is discarded.
const CONTACT_LIMIT: usize = 8;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WallInput {
    centre: (f32, f32),
    size: (f32, f32),
    #[serde(default)]
    yaw: f32,
}

/// A validated static rectangle. Local X/Z size is full length/thickness in
/// metres; yaw follows rendering (+Z turns toward +X). File decoding uses the
/// same constructor as typed callers. Dimensions must be at least one cm and
/// every corner must fit inside the arena.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(try_from = "WallInput", into = "WallInput")]
pub struct Wall {
    centre: Vec2,
    half: Vec2,
    yaw: f32,
    x_axis: Vec2,
    z_axis: Vec2,
    bounds: Vec2,
}

impl TryFrom<WallInput> for Wall {
    type Error = &'static str;
    fn try_from(input: WallInput) -> Result<Self, Self::Error> {
        Self::new(input.centre.into(), input.size.into(), input.yaw)
    }
}

impl Wall {
    /// Validate shape and world extent before any scene can install it.
    pub fn new(centre: Vec2, size: Vec2, yaw: f32) -> Result<Self, &'static str> {
        if !centre.is_finite()
            || !size.is_finite()
            || !yaw.is_finite()
            || size.min_element() < 0.01
            || size.max_element() > ARENA_HALF * 2.0
        {
            return Err(
                "wall needs finite centre/yaw and dimensions of at least 0.01 m within the arena",
            );
        }
        let yaw = crate::angle::wrap(yaw);
        let (sin, cos) = yaw.sin_cos();
        let x_axis = Vec2::new(cos, -sin);
        let z_axis = Vec2::new(sin, cos);
        let half = size * 0.5;
        let bounds = x_axis.abs() * half.x + z_axis.abs() * half.y;
        if (centre.abs() + bounds).max_element() > ARENA_HALF {
            return Err("every wall corner must fit inside the arena");
        }
        Ok(Self {
            centre,
            half,
            yaw,
            x_axis,
            z_axis,
            bounds,
        })
    }
    /// Ground centre in world X/Z.
    pub fn centre(self) -> Vec2 {
        self.centre
    }
    /// Full local X/Z dimensions in metres.
    pub fn size(self) -> Vec2 {
        self.half * 2.0
    }
    /// Rotation about world Y, in radians.
    pub fn yaw(self) -> f32 {
        self.yaw
    }
    fn local(self, vector: Vec2) -> Vec2 {
        Vec2::new(vector.dot(self.x_axis), vector.dot(self.z_axis))
    }
    fn vector(self, vector: Vec2) -> Vec2 {
        self.x_axis * vector.x + self.z_axis * vector.y
    }

    // Exact circle/rectangle penetration, including a centre inside the solid.
    fn penetration(self, centre: Vec2, radius: f32) -> Option<(Vec2, f32)> {
        if ((centre - self.centre).abs() - self.bounds).max_element() > radius {
            return None;
        }
        let p = self.local(centre - self.centre);
        let nearest = p.clamp(-self.half, self.half);
        let offset = p - nearest;
        let distance = offset.length();
        if distance > 0.0 {
            return (distance < radius)
                .then(|| (self.vector(offset / distance), radius - distance));
        }
        let gaps = self.half - p.abs();
        let axis = usize::from(gaps.y < gaps.x);
        let mut normal = Vec2::ZERO;
        normal[axis] = if p[axis] < 0.0 { -1.0 } else { 1.0 };
        Some((self.vector(normal), gaps[axis] + radius))
    }

    // Initial overlap recovery must not eject a body outside the arena. If
    // the shortest exit is unavailable, choose the nearest in-bounds face.
    fn recovery(self, centre: Vec2, radius: f32) -> Option<(Vec2, f32)> {
        let (normal, depth) = self.penetration(centre, radius)?;
        let limit = ARENA_HALF - radius;
        if (centre + normal * (depth + SKIN)).abs().max_element() <= limit {
            return Some((normal, depth + SKIN));
        }
        let local = self.local(centre - self.centre);
        let mut best: Option<(Vec2, f32)> = None;
        for axis in 0..2 {
            for sign in [-1.0, 1.0] {
                let mut point = local.clamp(-self.half, self.half);
                point[axis] = sign * (self.half[axis] + radius + SKIN);
                let point = self.centre + self.vector(point);
                let difference = point - centre;
                let distance = difference.length();
                if point.abs().max_element() <= limit
                    && distance > 0.0
                    && best.is_none_or(|(_, old)| distance < old)
                {
                    best = Some((difference / distance, distance));
                }
            }
        }
        best
    }

    // Cast a circle against the rectangle's rounded expansion: four faces and
    // four circular corners. A square expansion would snag near the corners.
    fn sweep(self, start: Vec2, delta: Vec2, radius: f32) -> Option<WallHit> {
        let end = start + delta;
        let low = start.min(end) - Vec2::splat(radius);
        let high = start.max(end) + Vec2::splat(radius);
        if (low.cmpgt(self.centre + self.bounds) | high.cmplt(self.centre - self.bounds)).any() {
            return None;
        }
        let p = self.local(start - self.centre);
        let d = self.local(delta);
        let mut result: Option<WallHit> = None;
        for axis in 0..2 {
            let other = 1 - axis;
            for sign in [-1.0, 1.0] {
                if d[axis] * sign >= 0.0 {
                    continue;
                }
                let t = (sign * (self.half[axis] + radius) - p[axis]) / d[axis];
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }
                let mut point = p + d * t;
                if point[other].abs() > self.half[other] {
                    continue;
                }
                point[axis] = sign * (self.half[axis] + radius);
                let mut normal = Vec2::ZERO;
                normal[axis] = sign;
                self.offer(&mut result, t, point, normal);
            }
        }
        if radius > 0.0 && d.length_squared() > 0.0 {
            // Double precision avoids cancellation in fast, distant sweeps.
            let d64 = d.as_dvec2();
            for x in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    let corner = self.half * Vec2::new(x, z);
                    let offset = (p - corner).as_dvec2();
                    let a = d64.length_squared();
                    let b = offset.dot(d64);
                    let c = offset.length_squared() - f64::from(radius).powi(2);
                    let discriminant = b * b - a * c;
                    if discriminant < 0.0 {
                        continue;
                    }
                    let t = ((-b - discriminant.sqrt()) / a) as f32;
                    if !(0.0..=1.0).contains(&t) {
                        continue;
                    }
                    let point = p + d * t;
                    let offset = point - corner;
                    if offset.x * x < 0.0 || offset.y * z < 0.0 {
                        continue;
                    }
                    let normal = offset.normalize_or_zero();
                    if d.dot(normal) >= 0.0 {
                        continue;
                    }
                    self.offer(&mut result, t, corner + normal * radius, normal);
                }
            }
        }
        result
    }
    // Share segment intersection between nearest-hit beam clipping and the
    // early-exit cover query. An origin inside the solid hits immediately.
    fn ray_hit(self, start: Vec2, delta: Vec2) -> Option<WallHit> {
        let offset = start - self.centre;
        if offset.abs().cmple(self.bounds).all() && self.local(offset).abs().cmple(self.half).all()
        {
            return Some(WallHit {
                t: 0.0,
                point: start,
                normal: Vec2::ZERO,
            });
        }
        // Intersect the two local slabs directly. Reconstructing a candidate
        // face point and testing the other coordinate can round an exact
        // corner hit outside both faces, letting a ray through a rotated box.
        let p = self.local(offset).as_dvec2();
        let d = self.local(delta).as_dvec2();
        let half = self.half.as_dvec2();
        let mut enter = 0.0_f64;
        let mut leave = 1.0_f64;
        let mut normal = Vec2::ZERO;
        for axis in 0..2 {
            if d[axis] == 0.0 {
                if p[axis].abs() > half[axis] { return None; }
                continue;
            }
            let a = (-half[axis] - p[axis]) / d[axis];
            let b = (half[axis] - p[axis]) / d[axis];
            let near = a.min(b);
            if near > enter {
                enter = near;
                normal = Vec2::ZERO;
                normal[axis] = -d[axis].signum() as f32;
            }
            leave = leave.min(a.max(b));
            if enter > leave { return None; }
        }
        let t = enter as f32;
        Some(WallHit { t, point: start + delta * t, normal: self.vector(normal) })
    }

    fn offer(self, result: &mut Option<WallHit>, t: f32, point: Vec2, normal: Vec2) {
        if result.is_none_or(|hit| t < hit.t) {
            *result = Some(WallHit {
                t,
                point: self.centre + self.vector(point),
                normal: self.vector(normal),
            });
        }
    }
    fn hash(self, out: &mut Fnv) {
        out.f32(self.centre.x);
        out.f32(self.centre.y);
        out.f32(self.half.x);
        out.f32(self.half.y);
        out.f32(self.yaw);
    }
    pub(crate) fn report(self, out: &mut Report) {
        out.vec3("centre", crate::on_ground(self.centre, 0.0));
        out.vec3("size", crate::on_ground(self.size(), 0.0));
        out.num("yaw", self.yaw);
    }
}


/// First ground-plane contact with a wall along a segment. Only the geometry
/// owner can construct it; a zero normal means the segment starts inside solid.
#[derive(Clone, Copy, Debug)]
pub struct WallHit {
    t: f32,
    pub(crate) point: Vec2,
    pub(crate) normal: Vec2,
}

impl WallHit {
    /// Fraction along the queried segment, from zero through one.
    pub fn fraction(self) -> f32 {
        self.t
    }
    /// Ground-plane contact point in world X/Z.
    pub fn point(self) -> Vec2 {
        self.point
    }
    /// Outward unit face normal, or zero for an origin inside solid.
    pub fn normal(self) -> Vec2 {
        self.normal
    }
}

/// Geometry stays in scene creation/authored order. The pair (scene, index)
/// is stable across eviction of other scenes; walls have no mutable body row.
struct OwnedWall {
    owner: SceneId,
    index: usize,
    wall: Wall,
}

pub(crate) struct Walls {
    rows: Vec<OwnedWall>,
}
impl Default for Walls {
    fn default() -> Self {
        Self {
            rows: Vec::with_capacity(MAX_WALLS),
        }
    }
}
impl Walls {
    pub(crate) fn admit(&self, count: usize) -> Result<(), SceneError> {
        if count > MAX_WALLS - self.rows.len() {
            return Err(SceneError::Invalid("live wall capacity exceeded"));
        }
        Ok(())
    }
    pub(crate) fn install(&mut self, owner: SceneId, walls: &[Wall]) {
        self.admit(walls.len())
            .expect("wall admission precedes installation");
        self.rows.extend(
            walls
                .iter()
                .copied()
                .enumerate()
                .map(|(index, wall)| OwnedWall { owner, index, wall }),
        );
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|row| row.owner != owner);
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (SceneId, usize, Wall)> + '_ {
        self.rows.iter().map(|row| (row.owner, row.index, row.wall))
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for row in &self.rows {
            row.owner.hash_into(out);
            out.usize(row.index);
            row.wall.hash(out);
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        for row in &self.rows {
            out.object(&format!("{}/{}", row.owner, row.index), |out| {
                row.wall.report(out)
            });
        }
    }
    /// Earliest surface on the centre ray, including an origin inside a wall.
    pub(crate) fn first_hit(&self, start: Vec2, end: Vec2) -> Option<WallHit> {
        let delta = end - start;
        self.rows
            .iter()
            .filter_map(|row| row.wall.ray_hit(start, delta))
            .min_by(|a, b| a.t.total_cmp(&b.t))
    }

    pub(crate) fn obstructs(&self, start: Vec2, end: Vec2) -> bool {
        let delta = end - start;
        self.rows.iter().any(|row| {
            row.wall
                .ray_hit(start, delta)
                .is_some_and(|hit| hit.t < 1.0)
        })
    }

    /// Resolve initial overlap, then sweep the entire displacement and slide.
    /// Returns the position and reports each outward contact normal to physics.
    pub(crate) fn slide(
        &self,
        start: Vec2,
        end: Vec2,
        radius: f32,
        mut contact: impl FnMut(Vec2),
    ) -> Vec2 {
        if self.is_empty() {
            return end;
        }
        let limit = Vec2::splat(ARENA_HALF - radius);
        let mut position = start.clamp(-limit, limit);
        // Spawn/additive scene loads may begin overlapped. Resolve those before
        // sweeping. Fixed authored props never call this movement operation.
        for _ in 0..CONTACT_LIMIT {
            let mut corrected = false;
            for row in &self.rows {
                if let Some((normal, depth)) = row.wall.recovery(position, radius) {
                    position = (position + normal * depth).clamp(-limit, limit);
                    contact(normal);
                    corrected = true;
                }
            }
            if !corrected {
                break;
            }
        }
        let mut remaining = end - position;
        for _ in 0..CONTACT_LIMIT {
            if remaining == Vec2::ZERO {
                return position;
            }
            let mut nearest = arena_hit(position, remaining, limit.x);
            for row in &self.rows {
                if let Some(hit) = row.wall.sweep(position, remaining, radius)
                    && nearest.is_none_or(|old| hit.t < old.t)
                {
                    nearest = Some(hit);
                }
            }
            let Some(hit) = nearest else {
                return position + remaining;
            };
            position = hit.point + hit.normal * SKIN;
            remaining *= 1.0 - hit.t;
            remaining -= hit.normal * remaining.dot(hit.normal).min(0.0);
            contact(hit.normal);
        }
        position
    }
}

// Sliding along a rotated rectangle can redirect a displacement toward an
// arena edge even when its originally requested endpoint was inside. Include
// arena faces in the same earliest-contact decision to preserve both solids.
fn arena_hit(start: Vec2, delta: Vec2, limit: f32) -> Option<WallHit> {
    let mut nearest: Option<WallHit> = None;
    for axis in 0..2 {
        if delta[axis] == 0.0 {
            continue;
        }
        let sign = delta[axis].signum();
        let t = (sign * limit - start[axis]) / delta[axis];
        if (0.0..=1.0).contains(&t) && nearest.is_none_or(|old| t < old.t) {
            let mut point = start + delta * t;
            point[axis] = sign * limit;
            let mut normal = Vec2::ZERO;
            normal[axis] = -sign;
            nearest = Some(WallHit { t, point, normal });
        }
    }
    nearest
}


impl From<Wall> for WallInput {
    fn from(wall: Wall) -> Self { Self { centre: wall.centre.into(), size: (wall.half * 2.0).into(), yaw: wall.yaw } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;

    fn wall() -> Wall {
        Wall::new(Vec2::ZERO, Vec2::new(4.0, 2.0), 0.0).unwrap()
    }
    fn walls(wall: Wall) -> Walls {
        let world = World::from_scene(&crate::Scene::default()).unwrap();
        let owner = world.scene_instances().next().unwrap().0;
        let mut walls = Walls::default();
        walls.install(owner, &[wall]);
        walls
    }
    #[test]
    fn rays_through_rotated_corners_hit_and_adjacent_parallel_rays_miss() {
        let centre = Vec2::new(0.0, 2.0);
        let wall = Wall::new(centre, Vec2::splat(0.72), std::f32::consts::FRAC_PI_4).unwrap();
        let edge = 0.36 * 2.0_f32.sqrt();
        let hit = wall.ray_hit(Vec2::ZERO, Vec2::new(0.0, 4.0)).unwrap();
        assert!((hit.point().y - (2.0 - edge)).abs() < 0.00001);
        assert!(wall.ray_hit(Vec2::new(edge + 0.0001, 0.0), Vec2::new(0.0, 4.0)).is_none());
        assert!(wall.ray_hit(Vec2::new(0.0, 4.0), Vec2::new(0.0, -4.0)).is_some());
    }

    #[test]
    fn face_corner_and_rotated_sweeps_finish_outside_the_solid() {
        for yaw in [0.0, 0.3, std::f32::consts::FRAC_PI_4, 1.5] {
            let shape = Wall::new(Vec2::new(0.3, -0.2), Vec2::new(4.0, 0.1), yaw).unwrap();
            let walls = walls(shape);
            for x in -3..=3 {
                for z in -3..=3 {
                    let start = Vec2::new(x as f32 * 2.0, z as f32 * 2.0);
                    for dx in -2..=2 {
                        for dz in -2..=2 {
                            let end = start + Vec2::new(dx as f32 * 5.0, dz as f32 * 5.0);
                            let result = walls.slide(start, end, 0.3, |_| {});
                            assert!(result.is_finite());
                            assert!(
                                shape
                                    .penetration(result, 0.3)
                                    .is_none_or(|(_, depth)| depth < SKIN),
                                "yaw={yaw} start={start} end={end} result={result}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rectangle_validation_refuses_nonfinite_degenerate_and_outside_geometry() {
        for (centre, size, yaw) in [
            (Vec2::splat(f32::NAN), Vec2::ONE, 0.0),
            (Vec2::ZERO, Vec2::splat(f32::INFINITY), 0.0),
            (Vec2::ZERO, Vec2::new(1.0, 0.0), 0.0),
            (Vec2::ZERO, Vec2::new(-1.0, 1.0), 0.0),
            (Vec2::ZERO, Vec2::ONE, f32::NAN),
            (Vec2::splat(96.0), Vec2::ONE, 0.0),
            (Vec2::new(95.4, 0.0), Vec2::ONE, std::f32::consts::FRAC_PI_4),
        ] {
            assert!(Wall::new(centre, size, yaw).is_err());
        }
    }
    #[test]
    fn rounded_corners_have_exact_normals_and_do_not_use_square_expansion() {
        let wall = wall();
        let diagonal = Vec2::ONE.normalize();
        let corner = Vec2::new(2.0, 1.0);
        let (normal, depth) = wall.penetration(corner + diagonal * 0.2, 0.3).unwrap();
        assert!(normal.abs_diff_eq(diagonal, 0.00001));
        assert!((depth - 0.1).abs() < 0.00001);
        assert!(wall.penetration(corner + Vec2::splat(0.25), 0.3).is_none());
        // Horizontal movement through the expanded square's corner, outside
        // the actual rounded corner, must be unobstructed.
        assert!(
            wall.sweep(Vec2::new(2.5, 1.29), Vec2::new(-0.3, 0.0), 0.3)
                .is_none()
        );
        let hit = wall.sweep(corner + diagonal, -diagonal * 2.0, 0.3).unwrap();
        assert!((hit.t - 0.35).abs() < 0.00001);
        assert!(hit.normal.abs_diff_eq(diagonal, 0.00001));
    }
    #[test]
    fn embedded_centres_escape_nearest_face_and_corner_sweeps_slide() {
        let walls = walls(wall());
        let embedded = Vec2::new(0.0, 0.8);
        let escaped = walls.slide(embedded, embedded, 0.3, |_| {});
        assert!(escaped.abs_diff_eq(Vec2::new(0.0, 1.3), 0.0001));
        assert!(walls.rows[0].wall.penetration(escaped, 0.3).is_none());
        let start = Vec2::new(-3.0, 1.15);
        let mut normals = Vec::new();
        let slid = walls.slide(start, Vec2::new(0.0, 1.15), 0.3, |n| normals.push(n));
        assert!(!normals.is_empty());
        assert!(
            slid.y > 1.3 && slid.x > -2.0,
            "slides around the rounded corner: {slid}"
        );
        assert!(walls.rows[0].wall.penetration(slid, 0.3).is_none());
    }
    #[test]
    fn ray_selects_nearest_surface_and_handles_parallel_and_inside_origins() {
        let walls = walls(wall());
        assert_eq!(
            walls
                .first_hit(Vec2::new(0.0, -3.0), Vec2::new(0.0, 3.0))
                .unwrap()
                .point,
            Vec2::new(0.0, -1.0)
        );
        assert_eq!(
            walls
                .first_hit(Vec2::ZERO, Vec2::new(0.0, 3.0))
                .unwrap()
                .point,
            Vec2::ZERO
        );
        assert!(
            walls
                .first_hit(Vec2::new(3.0, -3.0), Vec2::new(3.0, 3.0))
                .is_none()
        );
        assert!(!walls.obstructs(Vec2::new(0.0, -3.0), Vec2::new(0.0, -2.0)));
    }
}
