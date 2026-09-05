//! A stable hash over simulation state.
//!
//! Comparing two runs by their final positions tells you *that* they diverged.
//! Comparing a hash every tick tells you *which tick* — which is the difference
//! between "the replay is broken" and a bisect down to the one pass that
//! stopped being a pure function of its inputs.
//!
//! Hand-rolled FNV-1a rather than `DefaultHasher` for one reason: std's default
//! hasher is explicitly documented as unspecified and free to change between
//! releases. That is fine for a `HashMap` and disqualifying here, because a
//! golden trace checked into the repository has to still mean the same thing
//! after a toolchain bump. FNV-1a is a published constant-defined algorithm, so
//! the number is a property of the state rather than of the compiler.
//!
//! It is not cryptographic and does not need to be. The threat model is a
//! typo, not an adversary.

/// Offset basis and prime for 64-bit FNV-1a.
const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Accumulates simulation state into a single comparable number.
///
/// Floats go in as their **bit patterns**, deliberately. Comparing `f32`s by
/// value would treat `0.0` and `-0.0` as equal and every `NaN` as unequal to
/// itself; a determinism check wants neither. It wants to know whether two runs
/// produced the same bits, and any answer softer than that hides exactly the
/// class of drift it exists to find.
#[derive(Clone, Copy, Debug)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(OFFSET_BASIS)
    }
}

impl Fnv {
    /// Feeds eight bytes, little-endian.
    #[inline]
    pub fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(PRIME);
        }
    }

    /// Feeds a count. Length is state: two worlds with the same positions but
    /// different horde sizes are different worlds.
    #[inline]
    pub fn usize(&mut self, value: usize) {
        self.u64(value as u64);
    }

    /// Feeds a float by its bit pattern. See the type's own docs for why.
    #[inline]
    pub fn f32(&mut self, value: f32) {
        self.u64(u64::from(value.to_bits()));
    }

    /// The accumulated hash.
    #[must_use]
    pub fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_hash_is_the_offset_basis() {
        assert_eq!(Fnv::default().finish(), OFFSET_BASIS);
    }

    /// The property the whole determinism gate rests on. Without it a `hash()`
    /// that returned a constant would pass every replay test ever written.
    #[test]
    fn different_input_gives_a_different_hash() {
        let of = |v: f32| {
            let mut h = Fnv::default();
            h.f32(v);
            h.finish()
        };

        assert_ne!(of(1.0), of(1.000_001), "a one-ulp difference vanished");
        assert_ne!(of(0.0), of(-0.0), "sign of zero was folded away; bit patterns must survive");
    }

    /// Order has to matter, or a body swapping places with another would go
    /// unnoticed — which is precisely what a broadphase bug looks like.
    #[test]
    fn order_changes_the_hash() {
        let mut a = Fnv::default();
        a.f32(1.0);
        a.f32(2.0);

        let mut b = Fnv::default();
        b.f32(2.0);
        b.f32(1.0);

        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn the_same_input_gives_the_same_hash() {
        let mut a = Fnv::default();
        let mut b = Fnv::default();
        for i in 0..64 {
            a.f32(i as f32);
            b.f32(i as f32);
        }
        assert_eq!(a.finish(), b.finish());
    }
}
