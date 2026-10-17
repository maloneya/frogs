//! Per-frame skinning shared by every placement of a bounded horde pose.

use arpg_assets::{CharacterAssetId, CharacterPose, CharacterVertex, JointMatrix};
use glam::Mat4;

use super::{CharacterMesh, MAX_HORDE_POSE_BUCKETS};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

const _: () = assert!(size_of::<Vertex>() == 32);

pub(super) fn visible_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 5 => Float32x2];
    wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

pub(super) fn depth_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
    wgpu::VertexBufferLayout {
        array_stride: size_of::<[f32; 3]>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

fn skin(vertex: CharacterVertex, joints: &[JointMatrix]) -> Mat4 {
    let [a, b, c, d] = vertex.joints().map(usize::from);
    let [wa, wb, wc, wd] = vertex.weights();
    joints[a].matrix() * wa + joints[b].matrix() * wb
        + joints[c].matrix() * wc + joints[d].matrix() * wd
}

fn pose_vertex(vertex: CharacterVertex, joints: &[JointMatrix]) -> Vertex {
    let matrix = skin(vertex, joints);
    Vertex {
        position: (matrix * vertex.position().extend(1.0)).truncate().to_array(),
        // Instance inverse-scale correction and normalization remain in WGSL.
        normal: (matrix * vertex.normal().extend(0.0)).truncate().to_array(),
        uv: vertex.uv().to_array(),
    }
}

pub(super) struct Horde {
    asset_id: CharacterAssetId,
    visible: wgpu::Buffer,
    depth: wgpu::Buffer,
    visible_stride: u64,
    depth_stride: u64,
    vertices: Vec<Vertex>,
    positions: Vec<[f32; 3]>,
}

impl Horde {
    fn strides(mesh: &CharacterMesh) -> (u64, u64) {
        let caster_count = mesh.shadow.as_ref().map_or(mesh.source.len(), |shadow| shadow.source.len());
        ((mesh.source.len() * size_of::<Vertex>()) as u64,
            (caster_count * size_of::<[f32; 3]>()) as u64)
    }

    pub(super) fn new(device: &wgpu::Device, mesh: &CharacterMesh) -> Self {
        let (visible_stride, depth_stride) = Self::strides(mesh);
        let buffer = |label, stride| device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            // Admission caps bound these to 160 MiB and 60 MiB respectively.
            size: stride * MAX_HORDE_POSE_BUCKETS as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            asset_id: mesh.asset_id.clone(),
            visible: buffer("shared posed horde vertices", visible_stride),
            depth: buffer("shared posed horde caster positions", depth_stride),
            visible_stride, depth_stride,
            vertices: Vec::with_capacity(mesh.source.len()),
            positions: Vec::with_capacity(depth_stride as usize / size_of::<[f32; 3]>()),
        }
    }

    pub(super) fn fits(&self, mesh: &CharacterMesh, pose: &CharacterPose) -> bool {
        // Attaching a caster preserves import identity but can change the
        // extents of a second uploaded mesh from that same import.
        self.asset_id.owns(pose) && (self.visible_stride, self.depth_stride) == Self::strides(mesh)
    }

    pub(super) fn upload(&mut self, queue: &wgpu::Queue, mesh: &CharacterMesh,
        pose: &CharacterPose, bucket: usize) {
        let joints = pose.joint_matrices();
        self.vertices.clear();
        self.vertices.extend(mesh.source.iter().map(|&vertex| pose_vertex(vertex, joints)));
        self.positions.clear();
        if let Some(shadow) = &mesh.shadow {
            self.positions.extend(shadow.source.iter().map(|&vertex|
                (skin(vertex, joints) * vertex.position().extend(1.0)).truncate().to_array()));
        } else {
            self.positions.extend(self.vertices.iter().map(|vertex| vertex.position));
        }
        queue.write_buffer(&self.visible, bucket as u64 * self.visible_stride,
            bytemuck::cast_slice(&self.vertices));
        queue.write_buffer(&self.depth, bucket as u64 * self.depth_stride,
            bytemuck::cast_slice(&self.positions));
    }

    pub(super) fn visible(&self, bucket: usize) -> wgpu::BufferSlice<'_> {
        self.visible.slice(bucket as u64 * self.visible_stride..(bucket as u64 + 1) * self.visible_stride)
    }

    pub(super) fn depth(&self, bucket: usize) -> wgpu::BufferSlice<'_> {
        self.depth.slice(bucket as u64 * self.depth_stride..(bucket as u64 + 1) * self.depth_stride)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_admitted_vertex_and_bucket_fits_two_vertex_buffers() {
        let visible = arpg_assets::MAX_VERTICES * size_of::<Vertex>() * MAX_HORDE_POSE_BUCKETS;
        let depth = arpg_assets::MAX_VERTICES * size_of::<[f32; 3]>() * MAX_HORDE_POSE_BUCKETS;
        assert_eq!(visible, 160 << 20);
        assert_eq!(depth, 60 << 20);
        assert!(visible as u64 <= wgpu::Limits::default().max_buffer_size);
        assert!(depth as u64 <= wgpu::Limits::default().max_buffer_size);
    }
}
