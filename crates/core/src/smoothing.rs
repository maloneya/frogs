//! Frame-rate independent smoothing.
//!
//! This lives in `core` — reachable from both halves — for a reason that is
//! about people rather than architecture. The wrong way to smooth a value is
//! one of the most familiar lines in games programming:
//!
//! ```text
//! pos = lerp(pos, target, 0.1);   // once a frame
//! ```
//!
//! Anyone reaching for smoothing will write that from memory unless something
//! better is already in scope. So the correct primitive is put where it will be
//! found: the next thing that needs easing — hitstop decay, knockback falloff,
//! a health bar, a camera — should import [`damp`] rather than reinvent the
//! bug. Making the right thing the easy thing beats forbidding the wrong one.

use glam::Vec3;

/// Moves `from` half of the remaining distance to `to` every `half_life`
/// seconds, **no matter how often it is called**.
///
/// Why the familiar version is wrong: `lerp(from, to, 0.1)` once a frame keeps
/// 90% of the error *per frame* rather than per second. After one second that
/// leaves `0.9^60 ≈ 0.002` at 60Hz but `0.9^144 ≈ 3e-7` at 144Hz — the same code
/// converging thousands of times faster purely because the machine is faster.
/// The game then feels different on different hardware, and in this project it
/// would feel different depending on whether `V` had been pressed, corrupting
/// the measurement that key exists to take.
///
/// `2^(-dt/half_life)` composes exactly under subdivision, because
/// `2^(-a/h) · 2^(-b/h) = 2^(-(a+b)/h)`. Fifty steps of 10ms therefore land
/// exactly where one step of 500ms does.
///
/// Half-life is also a number a person can hold: "closes half the gap every
/// 0.12s" means something on its own, where a per-frame coefficient does not.
///
/// `half_life` must be positive. Zero divides, and a negative value inverts the
/// exponent so the error *grows* by a constant factor every frame — diverging to
/// infinity and then to NaN, silently. Callers hold this with a const assert on
/// the constant they pass; see `FOLLOW_HALF_LIFE` in `arpg-gfx`.
pub fn damp(from: f32, to: f32, half_life: f32, dt: f32) -> f32 {
    debug_assert!(half_life > 0.0, "a non-positive half-life diverges");
    to + (from - to) * (-dt / half_life).exp2()
}

/// [`damp`] applied componentwise. The exponential factor is a scalar, so this
/// is the same curve applied to a vector, not a different one.
pub fn damp_vec3(from: Vec3, to: Vec3, half_life: f32, dt: f32) -> Vec3 {
    debug_assert!(half_life > 0.0, "a non-positive half-life diverges");
    to + (from - to) * (-dt / half_life).exp2()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The property the whole thing exists for.** One coarse step and many
    /// fine steps covering the same wall-clock time must agree, or the value is
    /// smoothed differently on a faster machine.
    #[test]
    fn damping_is_frame_rate_independent() {
        let coarse = damp(0.0, 10.0, 0.25, 0.5);

        let mut fine = 0.0;
        for _ in 0..50 {
            fine = damp(fine, 10.0, 0.25, 0.01);
        }
        assert!((coarse - fine).abs() < 1e-4, "{coarse} vs {fine}");
    }

    /// The naive per-frame lerp this replaces, shown failing the same check —
    /// so the test states what is being avoided, not just what is wanted.
    #[test]
    fn the_naive_per_frame_lerp_is_not() {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;

        let coarse = lerp(0.0, 10.0, 0.1);
        let mut fine = 0.0;
        for _ in 0..50 {
            fine = lerp(fine, 10.0, 0.1);
        }
        assert!((coarse - fine).abs() > 8.0, "the naive form should diverge wildly");
    }

    /// A half-life is a claim with a number in it, so check the number.
    #[test]
    fn one_half_life_closes_half_the_distance() {
        assert!((damp(0.0, 10.0, 0.3, 0.3) - 5.0).abs() < 1e-4);
        assert!((damp(0.0, 10.0, 0.3, 0.6) - 7.5).abs() < 1e-4);
    }

    /// Exponential decay approaches and never passes. A spring would overshoot,
    /// which on a camera reads as seasickness.
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

    /// The vector form must be the scalar one applied per component.
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
