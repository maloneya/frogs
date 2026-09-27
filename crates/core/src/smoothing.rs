//! Elapsed-time half-life smoothing.

use glam::Vec3;

/// Closes half the distance from `from` to `to` every `half_life` seconds.
/// The exponential composes under time subdivision, unlike a per-frame lerp.
/// `half_life` must be positive; only debug builds assert this at runtime.
pub fn damp(from: f32, to: f32, half_life: f32, dt: f32) -> f32 {
    debug_assert!(half_life > 0.0, "a non-positive half-life diverges");
    to + (from - to) * (-dt / half_life).exp2()
}

/// [`damp`] applied componentwise.
pub fn damp_vec3(from: Vec3, to: Vec3, half_life: f32, dt: f32) -> Vec3 {
    debug_assert!(half_life > 0.0, "a non-positive half-life diverges");
    to + (from - to) * (-dt / half_life).exp2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damping_is_frame_rate_independent() {
        let coarse = damp(0.0, 10.0, 0.25, 0.5);

        let mut fine = 0.0;
        for _ in 0..50 {
            fine = damp(fine, 10.0, 0.25, 0.01);
        }
        assert!((coarse - fine).abs() < 1e-4, "{coarse} vs {fine}");
    }

    #[test]
    fn one_half_life_closes_half_the_distance() {
        assert!((damp(0.0, 10.0, 0.3, 0.3) - 5.0).abs() < 1e-4);
        assert!((damp(0.0, 10.0, 0.3, 0.6) - 7.5).abs() < 1e-4);
    }

    #[test]
    fn damping_never_overshoots() {
        let mut v = 0.0;
        for _ in 0..500 {
            let next = damp(v, 10.0, 0.05, 1.0 / 60.0);
            assert!(next >= v && next <= 10.0, "{next}");
            v = next;
        }
    }

    #[test]
    fn zero_elapsed_time_changes_nothing() {
        assert_eq!(damp(3.0, 10.0, 0.2, 0.0), 3.0);
        assert_eq!(damp_vec3(Vec3::X, Vec3::Y, 0.2, 0.0), Vec3::X);
    }

    #[test]
    fn the_vector_form_matches_the_scalar_one() {
        let v = damp_vec3(Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0), 0.2, 0.05);
        for (got, (from, to)) in
            [v.x, v.y, v.z].into_iter().zip([(1.0, 4.0), (2.0, 5.0), (3.0, 6.0)])
        {
            assert!((got - damp(from, to, 0.2, 0.05)).abs() < 1e-6);
        }
    }
}
