//! Shared GPU layout for per-instance transforms and tint.

use arpg_core::Instance;

/// How `arpg_core::Instance` is fed to the GPU.
///
/// This lives here rather than beside the type it describes because it names
/// wgpu, and `arpg-core` must not — otherwise `arpg-sim` would link the whole
/// graphics stack just to say where an enemy is standing.
///
/// Locations 0 and 1 belong to the mesh; instance data starts at 2.
const ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4];

/// The one line that makes this instance data rather than vertex data:
/// `step_mode: Instance` tells the GPU to advance this buffer once per
/// *instance* instead of once per vertex. Same buffer machinery, different
/// stepping rule — that's the whole trick.
pub(crate) fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<Instance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRS,
    }
}
