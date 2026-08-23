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
/// three trailing floats are deliberate headroom for things we already know are
/// coming — rotation, hit-flash intensity, team id — and a 48-byte stride keeps
/// the offset arithmetic trivial.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
/// The fields are private, and the padding is why. They are not storage anyone
/// should write — they are reserved space for rotation, hit-flash and team id,
/// and the 48-byte stride they produce is a contract with the vertex attribute
/// array and the shader. A struct literal could set `_pad0: 3.0` and ship
/// garbage to the GPU; `new` is the only door, and it zeroes them.
pub struct Instance {
    pos: [f32; 3],
    _pad0: f32,
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
    pub fn new(pos: Vec3, scale: Vec3, color: Vec3) -> Self {
        Self {
            pos: pos.into(),
            _pad0: 0.0,
            scale: scale.into(),
            _pad1: 0.0,
            color: color.into(),
            _pad2: 0.0,
        }
    }
}

/// Locations 0 and 1 belong to the mesh; instance data starts at 2.
const ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4];

/// The one line that makes this instance data rather than vertex data:
/// `step_mode: Instance` tells the GPU to advance this buffer once per
/// *instance* instead of once per vertex. Same buffer machinery, different
/// stepping rule — that's the whole trick.
pub fn layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<Instance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRS,
    }
}
