//! Conservative instance admission against the camera and active shadow views.
use glam::{Mat4, Vec3, Vec4};

#[derive(Clone, Copy)]
pub(crate) struct Frustum {
    planes: [(Vec4, f32); 6],
}

impl Frustum {
    pub(crate) fn new(matrix: Mat4) -> Self {
        let rows = matrix.transpose();
        let [x, y, z, w] = [rows.x_axis, rows.y_axis, rows.z_axis, rows.w_axis];
        Self {
            planes: [w + x, w - x, w + y, w - y, z, w - z]
                .map(|plane| (plane, plane.truncate().length())),
        }
    }

    pub(crate) fn contains_sphere(self, centre: Vec3, radius: f32) -> bool {
        if !centre.is_finite() || !radius.is_finite() {
            return true;
        }
        self.planes
            .iter()
            .all(|(plane, normal_length)| plane.dot(centre.extend(1.0)) + radius * normal_length >= 0.0)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    pub(crate) fn point(point: Vec3) -> Self {
        Self {
            min: point,
            max: point,
        }
    }
    pub(crate) fn include(&mut self, point: Vec3) {
        self.min = self.min.min(point);
        self.max = self.max.max(point);
    }
    pub(crate) fn corners(self) -> impl Iterator<Item = Vec3> {
        (0..8).map(move |i| {
            Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            )
        })
    }
    #[cfg(test)]
    pub(crate) fn contains_point(self, point: Vec3) -> bool {
        point.cmpge(self.min - Vec3::splat(0.00001)).all()
            && point.cmple(self.max + Vec3::splat(0.00001)).all()
    }

    pub(crate) fn visible(self, instance: arpg_core::Instance, views: &[Frustum]) -> bool {
        let scale = instance.scale();
        let rotation = glam::Quat::from_rotation_y(instance.yaw())
            * glam::Quat::from_rotation_x(instance.pitch());
        let centre = instance.pos() + rotation * ((self.min * 0.5 + self.max * 0.5) * scale);
        let radius = (self.max * 0.5 - self.min * 0.5).length() * scale.abs().max_element();
        views
            .iter()
            .any(|view| view.contains_sphere(centre, radius))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::{Instance, LinearRgb};
    #[test]
    fn whole_bounds_survive_edges_tilt_scale_and_the_directx_near_plane() {
        let view = Frustum::new(Mat4::IDENTITY);
        let bounds = Bounds {
            min: Vec3::splat(-0.2),
            max: Vec3::splat(0.2),
        };
        let instance = |p| Instance::new(p, Vec3::ONE, LinearRgb::WHITE);
        assert!(bounds.visible(instance(Vec3::new(1.15, 0.0, 0.5)), &[view]));
        assert!(!bounds.visible(instance(Vec3::new(2.0, 0.0, 0.5)), &[view]));
        assert!(bounds.visible(instance(Vec3::new(0.0, 0.0, -0.15)), &[view]));
        assert!(!bounds.visible(instance(Vec3::new(0.0, 0.0, -0.5)), &[view]));
        assert!(
            bounds.visible(
                Instance::new(Vec3::new(1.5, 0.0, 0.5), Vec3::splat(3.0), LinearRgb::WHITE)
                    .with_yaw(0.9)
                    .with_pitch(1.3),
                &[view]
            )
        );
        let other = Frustum::new(Mat4::from_translation(Vec3::new(-10.0, 0.0, 0.0)));
        assert!(
            bounds.visible(instance(Vec3::new(10.0, 0.0, 0.5)), &[view, other]),
            "offscreen shadow casters remain admitted"
        );
    }
}
