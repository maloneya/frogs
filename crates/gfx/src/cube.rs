use glam::Vec3;
use wgpu::util::DeviceExt;

use arpg_core::{Instance, MAX_INSTANCES};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
}

/// A unit cube centred on the origin: **24 vertices, not 8.**
///
/// Each face needs its own normal, and a vertex carries exactly one of each
/// attribute. So the eight geometric corners each appear three times, once per
/// face they belong to. That's the general rule — a vertex must be split
/// wherever *any* attribute is discontinuous across an edge, whether that's a
/// normal, a UV, or a colour. Only smooth-shaded meshes get to share.
fn cube_mesh() -> (Vec<Vertex>, Vec<u16>) {
    let normals = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];

    let mut verts = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    for n in normals {
        // Build an orthonormal basis around the face normal. Because
        // `t × b == n` by construction, walking the corners counter-clockwise
        // in the (t, b) plane also winds them counter-clockwise as seen from
        // *outside* the cube — which is what `front_face: Ccw` plus back-face
        // culling expects. Deriving this beats hand-writing 24 vertices and
        // getting exactly one face inside-out.
        let seed = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let t = (seed - n * n.dot(seed)).normalize();
        let b = n.cross(t);

        let base = verts.len() as u16;
        for (u, v) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = (n + t * u + b * v) * 0.5;
            verts.push(Vertex { pos: p.into(), normal: n.into() });
        }

        // Two triangles per quad, sharing the 0-2 diagonal.
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    (verts, indices)
}

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
fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<Instance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRS,
    }
}

/// The cube mesh on the GPU, plus the pipeline that draws it.
pub(crate) struct CubePipeline {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    instances: wgpu::Buffer,
}

impl CubePipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let (verts, indices) = cube_mesh();

        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cube pipeline layout"),
            bind_group_layouts: &[Some(camera_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cube pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                // Two buffers feeding one pipeline: the mesh, stepped per
                // vertex, and the instance data, stepped per instance. The GPU
                // walks them at different rates and hands the shader one row
                // from each.
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
                    }),
                    Some(instance_layout()),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(color_format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                // Half of every closed mesh faces away from the camera and can
                // never be seen. Culling it halves the triangles the rasteriser
                // has to consider, for free.
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (MAX_INSTANCES * size_of::<Instance>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            vertices,
            indices: index_buf,
            index_count: indices.len() as u32,
            instances,
        }
    }

    /// The second of the two places per frame where data crosses into the GPU.
    /// Returns how many instances are actually live.
    pub(crate) fn upload(&self, queue: &wgpu::Queue, instances: &[Instance]) -> u32 {
        let n = instances.len().min(MAX_INSTANCES);
        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances[..n]));
        n as u32
    }

    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera_bind_group: &wgpu::BindGroup,
        count: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);

        // One call. The whole horde. The last argument is the instance range —
        // everything else here is identical to drawing a single cube.
        pass.draw_indexed(0..self.index_count, 0, 0..count);
    }
}
