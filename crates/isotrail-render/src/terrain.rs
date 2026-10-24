use glam::Vec3;
use isotrail_bundle::GridInfo;
use wgpu::util::DeviceExt;

use crate::camera::slab_angle;
use crate::{Camera, Gpu, OffscreenTarget, TARGET_FORMAT, TerrainMesh, Vertex};

pub const BACKGROUND: [u8; 4] = [196, 214, 228, 255];
/// A late-morning sun, until lighting follows the hike's real time
pub const SUN_AZIMUTH_DEG: f64 = 150.0;
pub const SUN_ELEVATION_DEG: f64 = 40.0;
pub const AMBIENT: f32 = 0.35;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    /// Unit vector towards the sun, in the slab frame
    pub sun: Vec3,
    pub ambient: f32,
}

impl Lighting {
    pub fn fixed(grid: &GridInfo) -> Lighting {
        let azimuth = slab_angle(grid, SUN_AZIMUTH_DEG);
        let elevation = SUN_ELEVATION_DEG.to_radians() as f32;
        Lighting {
            sun: Vec3::new(
                azimuth.cos() * elevation.cos(),
                azimuth.sin() * elevation.cos(),
                elevation.sin(),
            ),
            ambient: AMBIENT,
        }
    }
}

/// A terrain mesh uploaded to the GPU
pub struct GpuMesh {
    vertices: wgpu::Buffer,
    count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [f32; 16],
    sun: [f32; 4],
}

/// Draws terrain meshes with flat-shaded Lambert lighting
pub struct TerrainRenderer {
    pipeline: wgpu::RenderPipeline,
}

impl TerrainRenderer {
    pub fn new(gpu: &Gpu) -> TerrainRenderer {
        let device = gpu.device();
        let module = device.create_shader_module(wgpu::include_wgsl!("terrain.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3,
                        1 => Float32x3,
                        2 => Unorm8x4,
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(TARGET_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        TerrainRenderer { pipeline }
    }

    pub fn upload(&self, gpu: &Gpu, mesh: &TerrainMesh) -> GpuMesh {
        let vertices = gpu
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        GpuMesh {
            vertices,
            count: mesh.vertices.len() as u32,
        }
    }

    /// Clears `target` to [`BACKGROUND`] and draws the mesh over it
    pub fn draw(
        &self,
        gpu: &Gpu,
        target: &OffscreenTarget,
        mesh: &GpuMesh,
        camera: &Camera,
        lighting: &Lighting,
    ) {
        let device = gpu.device();
        let uniforms = Uniforms {
            view_proj: camera.view_proj.to_cols_array(),
            sun: lighting.sun.normalize().extend(lighting.ambient).to_array(),
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain depth"),
            size: wgpu::Extent3d {
                width: target.width(),
                height: target.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

        let [r, g, b, a] = BACKGROUND.map(|c| f64::from(c) / 255.0);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.draw(0..mesh.count, 0..1);
        }
        gpu.queue().submit([encoder.finish()]);
    }
}
