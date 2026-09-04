//! Per-entity data on its way to the GPU, and the buffer it travels in.

use glam::Vec3;

/// Capacity of the instance buffer. Allocated once, up front.
///
/// Capacity and count are separate concerns: we size the buffer for the worst
/// case at startup and then only ever write the first N entries. Growing a GPU
/// buffer means allocating a new one and waiting for in-flight frames to stop
/// referencing the old one — not something to do mid-frame while tuning a dial.
pub const MAX_INSTANCES: usize = 200_000;

/// Per-entity data handed to the GPU. **This is the renderer's entire
/// vocabulary** — `gfx` knows about positions, scales and colours, and nothing
/// whatsoever about enemies, health, or attacks.
///
/// 48 bytes, laid out as three `vec4`s. It could be packed to 36 (vertex
/// buffers have no 16-byte alignment requirement, unlike uniforms), but the
/// trailing floats are deliberate headroom for things we already know are
/// coming, and a 48-byte stride keeps the offset arithmetic trivial. The first
/// of the three is now spent: `yaw` moved into it, which is exactly what the
/// headroom was for — the layout, the vertex attributes and the shader's
/// `@location` slots did not have to change to gain a rotation.
///
/// The remaining two are still reserved for hit-flash intensity and team id.
/// The fields are private, and the padding is why: they are reserved space, not
/// storage anyone should write. A struct literal could set `_pad1: 3.0` and ship
/// garbage to the GPU in a slot something is going to occupy later. `new` and
/// [`Instance::with_yaw`] are the only doors, and between them they write every
/// field that means something and zero every field that does not.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pos: [f32; 3],
    /// Rotation about the vertical axis, in radians. Shares a `vec4` with
    /// `pos`, so the shader reads it as `i_pos.w`.
    yaw: f32,
    scale: [f32; 3],
    _pad1: f32,
    color: [f32; 3],
    _pad2: f32,
}

// The 48-byte stride is a contract with three other places: the vertex
// attribute array below, the `@location` slots in shader.wgsl, and the buffer
// capacity maths. Rust can't see into WGSL, but it can at least refuse to
// compile if the Rust half drifts — which is the half that gets edited.
const _: () = assert!(size_of::<Instance>() == 48);
const _: () = assert!(size_of::<Instance>() == 3 * 4 * size_of::<f32>());

/// Guards against `MAX_INSTANCES` being raised past what's reasonable to
/// allocate up front. 200_000 x 48B is ~9.6MB; this trips well before anything
/// that would fail at startup on a real GPU.
const _: () = assert!(MAX_INSTANCES * size_of::<Instance>() < 64 << 20);

impl Instance {
    /// The only constructor, so the reserved padding is always zeroed.
    /// Unrotated; most things in the world have no meaningful facing.
    pub fn new(pos: Vec3, scale: Vec3, color: Vec3) -> Self {
        Self {
            pos: pos.into(),
            yaw: 0.0,
            scale: scale.into(),
            _pad1: 0.0,
            color: color.into(),
            _pad2: 0.0,
        }
    }

    /// Turns the instance about the vertical axis.
    ///
    /// **The convention, which `shader.wgsl` must match:** yaw `0` faces world
    /// `+Z`, and a positive yaw turns toward `+X` — so a direction maps to a
    /// yaw by `atan2(dir.x, dir.z)`. Rust cannot check that the WGSL agrees any
    /// more than it can check the vertex layout, so the two are written to be
    /// read together.
    pub fn with_yaw(mut self, yaw: f32) -> Self {
        self.yaw = yaw;
        self
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
    /// Silently drops anything past capacity. The budget is enforced upstream
    /// by `World::set_enemy_count`; this is the backstop for whatever emits
    /// instances next, which will not know that budget exists.
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

    /// The cap is a number, and no type can hold a number to a value. This is
    /// the backstop for the case the signature cannot cover: something emitting
    /// more instances than the GPU buffer was sized for.
    #[test]
    fn sink_stops_at_capacity() {
        let mut buf = InstanceBuffer::default();
        let mut sink = buf.sink();
        for _ in 0..MAX_INSTANCES + 1000 {
            sink.push(Instance::new(Vec3::ZERO, Vec3::ONE, Vec3::ZERO));
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
            sink.push(Instance::new(Vec3::ZERO, Vec3::ONE, Vec3::ZERO));
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
