//! Per-entity data on its way to the GPU, and the buffer it travels in.

use glam::Vec3;
use crate::LinearRgb;

/// Capacity of fixed instance buffers and the initial character allocation.
///
/// Fixed buffers reserve their worst case at startup and upload only live entries.
/// Character placements also include timed remains
/// from recycled body slots, so their renderer-owned buffer grows when needed.
pub const MAX_INSTANCES: usize = 200_000;

/// Per-placement GPU data, three vec4s. Yaw uses pos.w, white blend scale.w,
/// and local-X pitch the final float.
///
/// ```compile_fail,E0308
/// use arpg_core::{Instance, Srgb};
/// use glam::Vec3;
/// Instance::new(Vec3::ZERO, Vec3::ONE, Srgb::from_rgb8([128; 3]));
/// ```
///
/// ```compile_fail,E0308
/// use arpg_core::Instance;
/// use glam::Vec3;
/// Instance::new(Vec3::ZERO, Vec3::ONE, Vec3::ONE);
/// ```
///
/// GPU upload is one-way: byte reconstruction cannot bypass the constructor.
/// ```compile_fail,E0277
/// let _: arpg_core::Instance = bytemuck::cast([0.0_f32; 12]);
/// ```
#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
pub struct Instance {
    pos: [f32; 3],
    /// Rotation about the vertical axis, in radians. Shares a `vec4` with
    /// `pos`, so the shader reads it as `i_pos.w`.
    yaw: f32,
    scale: [f32; 3],
    white_mix: f32,
    color: [f32; 3],
    pitch: f32,
}

// The 48-byte stride is a contract with three other places: the vertex
// attribute array in gfx/instance.rs, the asset shaders' `@location` slots, and the buffer
// capacity maths. Rust can't see into WGSL, but it can at least refuse to
// compile if the Rust half drifts — which is the half that gets edited.
const _: () = assert!(size_of::<Instance>() == 48);
const _: () = assert!(size_of::<Instance>() == 3 * 4 * size_of::<f32>());

/// Guards against `MAX_INSTANCES` being raised past what's reasonable to
/// allocate up front. 200_000 x 48B is ~9.6MB; this trips well before anything
/// that would fail at startup on a real GPU.
const _: () = assert!(MAX_INSTANCES * size_of::<Instance>() < 64 << 20);

impl Instance {
    /// Constructs an upright instance with no white flash.
    /// Unrotated; most things in the world have no meaningful facing.
    /// `pos` is in metres; `scale` is a dimensionless mesh multiplier. For the
    /// authored mesh, its components multiply the imported dimensions.
    pub fn new(pos: Vec3, scale: Vec3, color: LinearRgb) -> Self {
        Self {
            pos: pos.into(),
            yaw: 0.0,
            scale: scale.into(),
            white_mix: 0.0,
            color: color.to_array(),
            pitch: 0.0,
        }
    }

    /// Yaw about Y: zero faces +Z, positive turns toward +X, matching the asset shaders.
    pub fn with_yaw(mut self, yaw: f32) -> Self {
        self.yaw = yaw;
        self
    }

    /// Tilts the instance about its local X axis before applying yaw.
    #[must_use]
    pub fn with_pitch(mut self, pitch: f32) -> Self {
        assert!(pitch.is_finite(), "pitch must be finite");
        self.pitch = pitch;
        self
    }

    /// Local X rotation in radians.
    #[must_use]
    pub fn pitch(&self) -> f32 { self.pitch }

    /// Blends the shaded surface toward unlit linear white without changing opacity.
    ///
    /// # Panics
    /// Panics unless the amount is finite and in `0..=1`.
    #[must_use]
    pub fn with_white_mix(mut self, amount: f32) -> Self {
        assert!(amount.is_finite() && (0.0..=1.0).contains(&amount),
            "white blend must be finite and between zero and one");
        self.white_mix = amount;
        self
    }

    /// Blend toward white, independent of textures and lighting.
    #[must_use]
    pub fn white_mix(&self) -> f32 { self.white_mix }

    /// World-space draw position.
    #[must_use]
    pub fn pos(&self) -> Vec3 {
        Vec3::from(self.pos)
    }

    /// Authored mesh scale, shared by drawing and conservative visibility bounds.
    #[must_use]
    pub fn scale(&self) -> Vec3 { Vec3::from(self.scale) }

    /// Which way this instance is turned, in radians. See [`Instance::with_yaw`]
    /// for the convention.
    #[must_use]
    pub fn yaw(&self) -> f32 {
        self.yaw
    }
}

/// The CPU-side staging buffer, allocated once at full capacity for the same
/// reason the GPU one is: growing it later means reallocating mid-frame.
///
/// It exists to hand out [`InstanceSink`] and nothing else. Note it never
/// exposes its `Vec`.
pub struct InstanceBuffer {
    buf: Vec<Instance>,
}

impl Default for InstanceBuffer {
    fn default() -> Self {
        Self { buf: Vec::with_capacity(MAX_INSTANCES) }
    }
}

impl InstanceBuffer {
    /// Hands out the only writer there is.
    ///
    /// Resetting happens *here*, on the way in, rather than at the top of
    /// whatever function does the filling. That is deliberate: "remember to
    /// clear the buffer first" is a rule someone eventually forgets, and the
    /// symptom — a buffer that grows without bound until the frame time climbs
    /// for no visible reason — points nowhere near the cause.
    pub fn sink(&mut self) -> InstanceSink<'_> {
        self.buf.clear();
        InstanceSink { remaining: MAX_INSTANCES, buf: &mut self.buf }
    }

    /// The frame's instances, ready to upload. Read-only: writing goes through
    /// [`InstanceBuffer::sink`].
    pub fn as_slice(&self) -> &[Instance] {
        &self.buf
    }
}

/// A write-only view of the instance buffer that can push at most
/// `MAX_INSTANCES` items and cannot do anything else.
///
/// This is the seam's whole vocabulary, and the narrowness is the point.
/// Handing out `&mut Vec<Instance>` instead would hand over the entire `Vec`
/// API, and with it four plausible-looking mistakes: `reserve` or `collect`
/// allocating every frame, pushing past the GPU buffer's capacity (which the
/// upload silently truncates, so the horde just stops growing), and forgetting
/// the reset. None of them are expressible here.
pub struct InstanceSink<'a> {
    buf: &'a mut Vec<Instance>,
    remaining: usize,
}

impl InstanceSink<'_> {
    /// Silently drops anything past capacity. Simulation admission enforces
    /// its body budget; this is the backstop for presentation producers.
    pub fn push(&mut self, instance: Instance) {
        if self.remaining == 0 {
            return;
        }
        self.buf.push(instance);
        self.remaining -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_blend_rejects_invalid_values_at_the_instance_boundary() {
        let instance = Instance::new(Vec3::ZERO, Vec3::ONE, LinearRgb::WHITE);
        assert_eq!(instance.white_mix(), 0.0);
        assert_eq!(instance.with_white_mix(1.0).white_mix(), 1.0);
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(std::panic::catch_unwind(|| instance.with_white_mix(value)).is_err());
        }
    }

    /// The cap is a number, and no type can hold a number to a value. This is
    /// the backstop for the case the signature cannot cover: something emitting
    /// more instances than the GPU buffer was sized for.
    #[test]
    fn sink_stops_at_capacity() {
        let mut buf = InstanceBuffer::default();
        let mut sink = buf.sink();
        for _ in 0..MAX_INSTANCES + 1000 {
            sink.push(Instance::new(Vec3::ZERO, Vec3::ONE, LinearRgb::BLACK));
        }
        assert_eq!(buf.as_slice().len(), MAX_INSTANCES);
    }

    /// A second frame must not append to the first. The reset lives in `sink()`
    /// precisely so no caller can skip it.
    #[test]
    fn sink_resets_between_frames() {
        let mut buf = InstanceBuffer::default();
        for _ in 0..3 {
            let mut sink = buf.sink();
            sink.push(Instance::new(Vec3::ZERO, Vec3::ONE, LinearRgb::BLACK));
        }
        assert_eq!(buf.as_slice().len(), 1);
    }

    /// The staging buffer is sized once up front, so a steady state — and any
    /// change in N below the cap — never reallocates.
    #[test]
    fn buffer_preallocates_full_capacity() {
        // Reaches the private field directly rather than widening the API with
        // a `capacity()` accessor that only a test would ever call.
        let buf = InstanceBuffer::default();
        assert!(buf.buf.capacity() >= MAX_INSTANCES);
    }
}
