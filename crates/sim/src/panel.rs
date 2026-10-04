//! Hinged rigid rectangles. The hinge removes translation; the latch holds the
//! remaining angular coordinate and fails once a tick's load exceeds its strength.
use crate::{Alpha, EntityId, Fnv, Wall};
use arpg_core::Report;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Maximum live articulated panels; independent of crowd population.
pub const MAX_PANELS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConfigInput {
    width: f32,
    thickness: f32,
    closed_yaw: f32,
    min_angle: f32,
    max_angle: f32,
    latch_impulse: f32,
}
/// Validated dimensions, angular stops and latch strength. Angles are radians;
/// latch strength is impulse in kg m/s at the edge opposite the hinge.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "ConfigInput", into = "ConfigInput")]
pub struct PanelConfig {
    width: f32,
    thickness: f32,
    closed_yaw: f32,
    min_angle: f32,
    max_angle: f32,
    latch_impulse: f32,
}
impl PanelConfig {
    /// Construct a panel whose closed angle lies between its stops.
    pub fn new(
        width: f32,
        thickness: f32,
        closed_yaw: f32,
        min_angle: f32,
        max_angle: f32,
        latch_impulse: f32,
    ) -> Result<Self, &'static str> {
        if ![
            width,
            thickness,
            closed_yaw,
            min_angle,
            max_angle,
            latch_impulse,
        ]
        .iter()
        .all(|n| n.is_finite())
            || !(0.1..=20.0).contains(&width)
            || !(0.01..=2.0).contains(&thickness)
            || thickness > width
            || min_angle > 0.0
            || max_angle < 0.0
            || min_angle >= max_angle
            || min_angle < -core::f32::consts::PI
            || max_angle > core::f32::consts::PI
            || latch_impulse <= 0.0
        {
            return Err(
                "panel needs width 0.1..=20m, thickness 0.01..=2m <= width, finite yaw, ordered stops containing zero within ±pi, and positive finite latch impulse",
            );
        }
        Ok(Self {
            width,
            thickness,
            closed_yaw: crate::angle::wrap(closed_yaw),
            min_angle,
            max_angle,
            latch_impulse,
        })
    }
    /// Full panel width in metres.
    pub fn width(self) -> f32 {
        self.width
    }
    /// Full panel thickness in metres.
    pub fn thickness(self) -> f32 {
        self.thickness
    }
    /// Closed orientation, radians about world Y.
    pub fn closed_yaw(self) -> f32 {
        self.closed_yaw
    }
    /// Lower permitted relative angle.
    pub fn min_angle(self) -> f32 {
        self.min_angle
    }
    /// Upper permitted relative angle.
    pub fn max_angle(self) -> f32 {
        self.max_angle
    }
    /// Failure threshold in impulse units at the latch.
    pub fn latch_impulse(self) -> f32 {
        self.latch_impulse
    }
    pub(crate) fn hinge(self, centre: Vec2) -> Vec2 {
        centre - axis(self.closed_yaw) * self.width * 0.5
    }
    /// Distance from the hinge to the farthest corner; bounds the swept area
    /// and converts angular speed into the fastest point speed.
    pub(crate) fn reach(self) -> f32 {
        self.reach_squared().sqrt()
    }
    pub(crate) fn reach_squared(self) -> f32 {
        self.width * self.width + (self.thickness * 0.5) * (self.thickness * 0.5)
    }
    pub(crate) fn valid_at(self, centre: Vec2) -> bool {
        (self.hinge(centre).abs() + Vec2::splat(self.reach())).max_element() <= crate::ARENA_HALF
    }
}
impl Default for PanelConfig {
    fn default() -> Self {
        Self::new(2.0, 0.18, 0.0, -1.55, 1.55, 12.0).expect("valid panel defaults")
    }
}
impl TryFrom<ConfigInput> for PanelConfig {
    type Error = &'static str;
    fn try_from(v: ConfigInput) -> Result<Self, Self::Error> {
        Self::new(
            v.width,
            v.thickness,
            v.closed_yaw,
            v.min_angle,
            v.max_angle,
            v.latch_impulse,
        )
    }
}
impl From<PanelConfig> for ConfigInput {
    fn from(v: PanelConfig) -> Self {
        Self {
            width: v.width,
            thickness: v.thickness,
            closed_yaw: v.closed_yaw,
            min_angle: v.min_angle,
            max_angle: v.max_angle,
            latch_impulse: v.latch_impulse,
        }
    }
}
/// Physical assembly bound to one explicit scene body.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    /// Index into the scene's explicit body placements.
    pub body: usize,
    /// Validated mechanical configuration.
    pub config: PanelConfig,
}
/// Attachment state, independent of how far the panel has swung.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum LatchState {
    /// Intact and holding closed.
    Latched,
    /// Released by an authorized mechanism.
    Released,
    /// Failed under physical load.
    Broken,
}
/// Immutable current mechanics of one hinged panel.
#[derive(Clone, Copy, Debug)]
pub struct PanelState {
    pub(crate) id: EntityId,
    pub(crate) config: PanelConfig,
    pub(crate) hinge: Vec2,
    pub(crate) angle: f32,
    pub(crate) previous: f32,
    pub(crate) omega: f32,
    pub(crate) inverse_inertia: f32,
    pub(crate) latch: LatchState,
    pub(crate) reaction: f32,
    /// Warm-start torques (N m) for the two angular stops.
    pub(crate) lower: f32,
    pub(crate) upper: f32,
}
impl PanelState {
    /// Stable body identity.
    pub fn id(self) -> EntityId {
        self.id
    }
    /// Physical dimensions and attachment tuning.
    pub fn config(self) -> PanelConfig {
        self.config
    }
    /// Fixed hinge position on the ground plane.
    pub fn hinge(self) -> Vec2 {
        self.hinge
    }
    /// Current panel centre on the ground plane.
    pub fn centre(self) -> Vec2 {
        self.hinge + axis(self.yaw()) * self.config.width * 0.5
    }
    /// Full width and thickness in metres.
    pub fn size(self) -> Vec2 {
        Vec2::new(self.config.width, self.config.thickness)
    }
    /// Absolute yaw in radians.
    pub fn yaw(self) -> f32 {
        self.config.closed_yaw + self.angle
    }
    /// Relative angle from closed.
    pub fn angle(self) -> f32 {
        self.angle
    }
    /// Angular velocity in radians per second.
    pub fn angular_velocity(self) -> f32 {
        self.omega
    }
    /// Current latch state.
    pub fn latch(self) -> LatchState {
        self.latch
    }
    /// Most recent tick's latch impulse, expressed at the edge opposite the
    /// hinge. A broken latch reports the strength it spent before failing.
    pub fn reaction_impulse(self) -> f32 {
        self.reaction
    }
    pub(crate) fn shape(self) -> Wall {
        Wall::new(self.centre(), self.size(), self.yaw())
            .expect("panel swing admitted inside arena")
    }
    pub(crate) fn arm(self, point: Vec2, normal: Vec2) -> f32 {
        lever(point - self.hinge, normal)
    }
    pub(crate) fn point_velocity(self, point: Vec2) -> Vec2 {
        spin_velocity(point - self.hinge, self.omega)
    }
}
/// Angular lever of a unit impulse along `normal` applied at `r` from a hinge.
pub(crate) fn lever(r: Vec2, normal: Vec2) -> f32 {
    r.y * normal.x - r.x * normal.y
}
/// Velocity of the point `r` from a hinge turning at `omega`.
pub(crate) fn spin_velocity(r: Vec2, omega: f32) -> Vec2 {
    Vec2::new(r.y, -r.x) * omega
}
fn axis(yaw: f32) -> Vec2 {
    let (s, c) = yaw.sin_cos();
    Vec2::new(c, -s)
}
/// Interpolated physical panel geometry for rendering.
pub type PanelPresentation = PanelState;

#[derive(Default)]
pub(crate) struct Panels {
    who: crate::members::Members,
    pub(crate) rows: Vec<PanelState>,
}
impl Panels {
    pub(crate) fn get(&self, id: EntityId) -> Option<PanelState> {
        self.index(id).map(|i| self.rows[i])
    }
    pub(crate) fn index(&self, id: EntityId) -> Option<usize> {
        self.who.index(id)
    }
    pub(crate) fn install(
        &mut self,
        id: EntityId,
        centre: Vec2,
        config: PanelConfig,
        inverse_mass: f32,
    ) {
        assert!(self.rows.len() < MAX_PANELS && self.get(id).is_none() && inverse_mass > 0.0);
        let inertia_per_mass =
            (4.0 * config.width * config.width + config.thickness * config.thickness) / 12.0;
        self.who.add(id);
        self.rows.push(PanelState {
            id,
            config,
            hinge: config.hinge(centre),
            angle: 0.0,
            previous: 0.0,
            omega: 0.0,
            inverse_inertia: inverse_mass / inertia_per_mass,
            latch: LatchState::Latched,
            reaction: 0.0,
            lower: 0.0,
            upper: 0.0,
        });
    }
    pub(crate) fn revoke(&mut self, id: EntityId) {
        if let Some(i) = self.who.remove(id) {
            self.rows.swap_remove(i);
        }
    }
    pub(crate) fn release(&mut self, id: EntityId) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        if self.rows[i].latch != LatchState::Latched {
            return false;
        }
        self.rows[i].latch = LatchState::Released;
        true
    }
    pub(crate) fn presentations(
        &self,
        alpha: Alpha,
    ) -> impl Iterator<Item = PanelPresentation> + '_ {
        self.rows.iter().copied().map(move |mut p| {
            p.angle = p.previous + (p.angle - p.previous) * alpha.get();
            p
        })
    }
    pub(crate) fn hash(&self, h: &mut Fnv) {
        h.usize(self.rows.len());
        for p in &self.rows {
            p.id.hash_into(h);
            for n in [
                p.config.width,
                p.config.thickness,
                p.config.closed_yaw,
                p.config.min_angle,
                p.config.max_angle,
                p.config.latch_impulse,
                p.hinge.x,
                p.hinge.y,
                p.angle,
                p.previous,
                p.omega,
                p.inverse_inertia,
                p.reaction,
                p.lower,
                p.upper,
            ] {
                h.f32(n);
            }
            h.usize(p.latch as usize);
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("panels", |out| {
            for p in &self.rows {
                out.object(&p.id.to_string(), |out| {
                    out.vec3("hinge", crate::on_ground(p.hinge, 0.0));
                    out.vec3("centre", crate::on_ground(p.centre(), 0.0));
                    out.num("width", p.config.width);
                    out.num("thickness", p.config.thickness);
                    out.num("closed_yaw", p.config.closed_yaw);
                    out.num("yaw", p.yaw());
                    out.num("min_angle", p.config.min_angle);
                    out.num("max_angle", p.config.max_angle);
                    out.num("latch_impulse", p.config.latch_impulse);
                    out.num("inverse_inertia", p.inverse_inertia);
                    out.num("previous_angle", p.previous);
                    out.num("angle", p.angle);
                    out.num("angular_velocity", p.omega);
                    out.num("reaction_impulse", p.reaction);
                    out.num("lower_stop_torque", p.lower);
                    out.num("upper_stop_torque", p.upper);
                    out.text("latch", &format!("{:?}", p.latch));
                });
            }
        });
    }
}
