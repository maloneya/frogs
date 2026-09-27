//! FNV-1a over simulation state for replay comparison.
//! The algorithm is explicit rather than std's unspecified `DefaultHasher`.
//! This is not a cryptographic hash or a cross-toolchain replay guarantee.

/// Offset basis and prime for 64-bit FNV-1a.
const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Hashes float bit patterns, distinguishing signed zero and preserving NaN bits.
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

}
